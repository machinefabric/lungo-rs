use async_store::{__meta, StoreOpHandler, copy, swap};
use lungo::ClaimStatus;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;

/// Runs `f` to completion on this thread, parking it while the future is pending.
fn block_on<F: Future>(f: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Unpark(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

/// A store in memory, which answers after yielding once (as a real store would, later).
#[derive(Default)]
struct Memory {
    data: Mutex<HashMap<String, String>>,
    operations: Mutex<Vec<String>>,
}

/// Yields to the executor once.
async fn later() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}

#[derive(Debug, PartialEq)]
struct Unavailable;

impl StoreOpHandler for Memory {
    type Error = Unavailable;

    async fn get(&self, key: String) -> Result<Option<String>, Unavailable> {
        later().await;
        self.operations.lock().unwrap().push(format!("get {key}"));
        Ok(self.data.lock().unwrap().get(&key).cloned())
    }

    async fn put(&self, key: String, value: String) -> Result<(), Unavailable> {
        later().await;
        if key == "readonly" {
            return Err(Unavailable);
        }
        self.operations.lock().unwrap().push(format!("put {key}"));
        self.data.lock().unwrap().insert(key, value);
        Ok(())
    }
}

fn store(entries: &[(&str, &str)]) -> Memory {
    let m = Memory::default();
    m.data.lock().unwrap().extend(entries.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    m
}

/// TEST0336: copy and swap do to a store what they do to the model
#[test]
fn test0336_copy_and_swap_do_to_a_store_what_they_do_to_the_model() {
    let m = store(&[("a", "1"), ("b", "2")]);
    assert_eq!(block_on(copy(&m, "a".into(), "c".into())), Ok(true));
    assert_eq!(block_on(copy(&m, "missing".into(), "d".into())), Ok(false));
    assert_eq!(block_on(swap(&m, "a".into(), "b".into())), Ok(true));
    assert_eq!(block_on(swap(&m, "a".into(), "missing".into())), Ok(false));
    let data = m.data.lock().unwrap().clone();
    let expected: HashMap<String, String> =
        [("a", "2"), ("b", "1"), ("c", "1")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(data, expected);
    assert_eq!(
        m.operations.lock().unwrap().join(", "),
        "get a, put c, get missing, get a, get b, put a, put b, get a, get missing"
    );
}

/// TEST0337: a store's error abandons the program
#[test]
fn test0337_a_stores_error_abandons_the_program() {
    let m = store(&[("a", "1")]);
    assert_eq!(block_on(copy(&m, "a".into(), "readonly".into())), Err(Unavailable));
    let claim = __meta::assurance().claim("Store.copy_model").unwrap();
    assert_eq!((claim.status, claim.specifications), (ClaimStatus::Proved, &["Store.Model"][..]));
    let facility = __meta::assurance().facility("Store.storeOps").unwrap();
    assert!(facility.asynchronous && facility.id == "store.kv");
    assert!(__meta::declaration("Store.copy").unwrap().assurance.asynchronous);
}
