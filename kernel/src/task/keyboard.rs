use conquer_once::spin::OnceCell;
use core::pin::Pin;
use core::task::{Context, Poll};
use crossbeam_queue::ArrayQueue;
use futures_util::stream::Stream;
use futures_util::task::AtomicWaker;
use pc_keyboard::KeyCode;

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

/// Called from the keyboard IRQ handler. Best-effort push: if the queue is
/// full we drop the scancode rather than block in interrupt context.
pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        let _ = queue.push(scancode);
        WAKER.wake();
    }
}

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE
            .try_init_once(|| ArrayQueue::new(100))
            .expect("ScancodeStream::new called more than once");
        Self { _private: () }
    }
}

impl Default for ScancodeStream {
    fn default() -> Self {
        Self::new()
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE
            .try_get()
            .expect("scancode queue not initialised");

        if let Some(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        WAKER.register(cx.waker());
        match queue.pop() {
            Some(scancode) => {
                WAKER.take();
                Poll::Ready(Some(scancode))
            }
            None => Poll::Pending,
        }
    }
}

/// Map a `pc_keyboard::KeyCode` (the "raw" non-Unicode side of `DecodedKey`)
/// to a framebuffer scrollback action. Called from `shell::shell_loop` for
/// each `DecodedKey::RawKey` event so PgUp/PgDn/End freeze the live tail and
/// walk through history.
pub fn route_rawkey(k: KeyCode) {
    let writer = match crate::FB_WRITER.get() {
        Some(w) => w,
        None => return,
    };
    let mut w = writer.lock();
    let step = w.viewport_rows().saturating_sub(1).max(1);
    match k {
        KeyCode::PageUp => w.scroll_up(step),
        KeyCode::PageDown => w.scroll_down(step),
        KeyCode::End => w.scroll_end(),
        _ => {}
    }
}
