//! The control port (issue 41): a window started with `--control PORT` listens on its own machine's address, and an agent that
//! `bimcraft-cli mcp --connect` works on the project the window has open, as the user does, rather than on a headless
//! copy of its own. The protocol, what an agent may ask and may not, and the file that has the port and the token are
//! `bimcraft_core::bridge`; this is the threads that listen and the frame that answers.
//!
//! The requests come from outside, so they are hostile: only this machine's address is bound, a connection that does not say the
//! token in its first line is dropped, a line is read to a limit, only a few connections are held, and a request that
//! is no request is an error reply. A request runs on the window's own thread, in the frame, as a command from a button does
//! (`BimApp::answer_agents`), so what the agent makes is the user's to see, to undo and to save.

use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bimcraft_core::bridge::{self, Entry, Error, MAX_LINE, Request};
use bimcraft_core::ids::Guid;
use egui::Context;

use crate::BimApp;

/// The most agents connected at once.
const MAX_CONNECTIONS: usize = 8;
/// How long a connection has to say its first line, with the token in it.
const FIRST_LINE: Duration = Duration::from_secs(10);
/// How long a request waits for the window to answer: a render or an import of a big model takes a while.
const ANSWER: Duration = Duration::from_secs(600);
/// How many requests a frame answers.
const PER_FRAME: usize = 16;

/// A request on its way to the window, and where its reply goes.
struct Asked {
    request: Request,
    reply: Sender<String>,
}

/// The listening port and what it has taken.
pub struct Control {
    /// Where agents find it: the port, and the token that lets them in.
    pub entry: Entry,
    asked: Receiver<Asked>,
    /// The window's context, once the first frame has set it, so that a request wakes the window.
    wake: Arc<OnceLock<Context>>,
    /// The bridge file written for it, taken away when the window goes.
    file: Option<PathBuf>,
}

impl Control {
    /// Listens on `port` of this machine's own address (0: any that is free) and writes the bridge `file` with the port and a token
    /// of 128 random bits, for the user alone.
    pub fn start(port: u16, file: Option<PathBuf>) -> io::Result<Control> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let entry = Entry { port: listener.local_addr()?.port(), token: Guid::new().to_string() };
        if let Some(file) = &file {
            bridge::write_entry(file, &entry)?;
        }
        let (tx, asked) = channel();
        let wake = Arc::new(OnceLock::new());
        let (token, woken, live) = (entry.token.clone(), Arc::clone(&wake), Arc::new(AtomicUsize::new(0)));
        std::thread::Builder::new().name("control-accept".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                // A ninth agent is turned away, not queued: it was dropped, which closes it.
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (token, tx, woken, live) = (token.clone(), tx.clone(), Arc::clone(&woken), Arc::clone(&live));
                let spawned = std::thread::Builder::new().name("control-agent".into()).spawn(move || {
                    let _ = serve(stream, &token, &tx, &woken);
                    live.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    break;
                }
            }
        })?;
        Ok(Control { entry, asked, wake, file })
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            bridge::remove_entry(file, &self.entry);
        }
    }
}

/// One line of at most [`MAX_LINE`] bytes, or none at the end of what the agent sends; a longer one is an error.
pub(crate) fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = String::new();
    let read = reader.by_ref().take(MAX_LINE as u64 + 1).read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if read > MAX_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a request line is too long"));
    }
    Ok(Some(line))
}

/// An agent's connection: a line in, the window's answer back, until it hangs up or says a wrong token.
fn serve(stream: TcpStream, token: &str, tx: &Sender<Asked>, wake: &OnceLock<Context>) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(FIRST_LINE))?;
    let mut reader = io::BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut first = true;
    while let Some(line) = read_line(&mut reader)? {
        if std::mem::take(&mut first) {
            // Once it is known, an agent may take as long as it likes between requests.
            writer.set_read_timeout(None)?;
        }
        let reply = match bridge::parse(&line, token) {
            Err(Error::Token) => {
                writeln!(writer, "{}", bridge::reply(Err(Error::Token.to_string())))?;
                return Ok(());
            }
            Err(e) => bridge::reply(Err(e.to_string())),
            Ok(request) => {
                let (reply, answer) = channel();
                if tx.send(Asked { request, reply }).is_err() {
                    // The window has gone.
                    return Ok(());
                }
                if let Some(ctx) = wake.get() {
                    ctx.request_repaint();
                }
                answer.recv_timeout(ANSWER).unwrap_or_else(|_| bridge::reply(Err("the window did not answer".into())))
            }
        };
        writeln!(writer, "{reply}")?;
        writer.flush()?;
    }
    Ok(())
}

impl BimApp {
    /// Lets agents work on this window's project, over `control`, and says so.
    pub fn use_control(&mut self, control: Control) {
        let port = control.entry.port.to_string();
        self.status = crate::Status {
            text: crate::lang::tf("An agent can work on this project: bimcraft-cli mcp --connect {}", &[&port]),
            error: false,
        };
        self.control = Some(control);
    }

    /// Once a frame: what agents asked of the project since the last one is done, each as a command from a button is, and
    /// answered. While a file is being read or an update installed the project is about to be replaced: they wait.
    pub(crate) fn answer_agents(&mut self, ctx: &Context) {
        let Some(control) = &self.control else { return };
        let _ = control.wake.set(ctx.clone());
        if self.importing() || self.installing() {
            return;
        }
        let asked: Vec<Asked> = std::iter::from_fn(|| control.asked.try_recv().ok()).take(PER_FRAME).collect();
        let more = asked.len() == PER_FRAME;
        for Asked { request, reply } in asked {
            self.made_in();
            let result = bridge::run(&mut self.session, &request);
            // The core renames and drops the option with its set.
            self.active_option = self.session.option.clone();
            let _ = reply.send(bridge::reply(result));
            ctx.request_repaint();
        }
        if more {
            ctx.request_repaint();
        }
    }
}
