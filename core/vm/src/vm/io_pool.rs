use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

enum Message {
    Run(Job),
    Shutdown,
}

pub struct IoPool {
    tx: Sender<Message>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl IoPool {
    pub fn new(worker_count: usize) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let (tx, _) = mpsc::channel::<Message>();
            Self {
                tx,
                workers: Vec::new(),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = mpsc::channel::<Message>();
            let shared_rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
            let mut workers = Vec::with_capacity(worker_count);
            for _ in 0..worker_count {
                let worker_rx = shared_rx.clone();
                workers.push(thread::spawn(move || loop {
                    let msg = {
                        let lock = worker_rx.lock();
                        if let Ok(guard) = lock {
                            guard.recv()
                        } else {
                            return;
                        }
                    };
                    match msg {
                        Ok(Message::Run(job)) => job(),
                        Ok(Message::Shutdown) | Err(_) => return,
                    }
                }));
            }
            Self { tx, workers }
        }
    }

    pub fn submit<F, T>(&self, work: F) -> Receiver<Result<T, String>>
    where
        F: FnOnce() -> Result<T, String> + Send + 'static,
        T: Send + 'static,
    {
        let (result_tx, result_rx) = mpsc::channel::<Result<T, String>>();
        let job = Box::new(move || {
            let _ = result_tx.send(work());
        });
        let _ = self.tx.send(Message::Run(job));
        result_rx
    }
}

impl Drop for IoPool {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            let _ = self.tx.send(Message::Shutdown);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
