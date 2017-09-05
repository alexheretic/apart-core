use std::thread;
use std::sync::mpsc;

pub fn receiver<T, F>(func: F)
    -> mpsc::Receiver<T>
    where T: Send + 'static,
          F: Send + 'static,
          F: FnOnce() -> T
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move|| {
        let result = func();
        if let Err(err) = sender.send(result) {
            warn!("ThreadTask forgotten: {:?}", err);
        }
    });
    receiver
}
