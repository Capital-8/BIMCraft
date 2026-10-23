//! The system's file dialogs. One run inside the window's event handler nests a run loop of its own in it, and on macOS
//! a drag that reaches the window meanwhile re-enters the handler and aborts the app (issue 43). So a dialog is taken on a
//! thread of its own, the window keeps its event loop, and the path comes back in a later frame.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::Context;

use crate::BimApp;

/// What is done with the path a dialog gives.
pub(crate) type Then = Box<dyn FnOnce(&mut BimApp, PathBuf)>;

/// A dialog that is open: what it answers, and what to do with a path.
pub(crate) struct Dialog {
    pub(crate) answer: Receiver<Option<PathBuf>>,
    pub(crate) then: Then,
}

impl BimApp {
    /// Asks for a file to open, or for where to save one (`save`), and calls `then` with the path in a later frame. Nothing
    /// happens if the dialog is cancelled, and a second dialog is not opened while one is.
    pub(crate) fn ask_file(
        &mut self,
        ctx: &Context,
        dialog: rfd::AsyncFileDialog,
        save: bool,
        then: impl FnOnce(&mut BimApp, PathBuf) + 'static,
    ) {
        if self.dialog.is_some() {
            return;
        }
        let (tx, answer) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let file = pollster::block_on(async { if save { dialog.save_file().await } else { dialog.pick_file().await } });
            let _ = tx.send(file.map(|f| f.path().to_path_buf()));
            ctx.request_repaint();
        });
        self.dialog = Some(Dialog { answer, then: Box::new(then) });
    }

    /// Once a frame: acts on the path an open dialog has given. A thread that died leaves no answer, and the dialog is dropped.
    pub(crate) fn dialog_answered(&mut self) {
        let Some(open) = &self.dialog else { return };
        match open.answer.try_recv() {
            Ok(path) => {
                if let Some(Dialog { then, .. }) = self.dialog.take()
                    && let Some(path) = path
                {
                    then(self, path);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.dialog = None,
        }
    }
}
