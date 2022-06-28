use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::executor::block_on;
use minitrace::prelude::*;

use smallstr::SmallString;

type String = SmallString<[u8; 128]>;

// {"ph":"E","pid":1,"ts":1534353.096,"name":"RpcAdapter check_transport_service: wc polled","cat":"koala::rpc_adapter::engine","tid":1,"args":{"[file]":"src/koala/src/rpc_adapter/engine.rs","[line]":358}},
use json::{number::Number, object::Object};

struct Event {
    ph: &'static str,
    pid: usize,
    ts: f64,
    name: String,
    cat: String,
    tid: u32,
    args: Object,
}

// TODO(cjr): set a separate switch for each runtime to reduce cache contention
static PROFILING_ENABLE: AtomicBool = AtomicBool::new(true);

macro_rules! enter_with_local_parent {
    ($event:expr) => {
        let _guard = if !PROFILING_ENABLE.load(Ordering::Relaxed) {
            None
        } else {
            let mut local_span_guard = LocalSpan::enter_with_local_parent($event);
            // local_span_guard.add_property(|| ("cat", String::from(module_path!())));
            // local_span_guard.add_property(|| ("[file]", String::from(file!())));
            // local_span_guard.add_property_static(("cat", module_path!()));
            // local_span_guard.add_property_static(("[file]", file!()));
            local_span_guard.add_properties_static(|| {
                [
                    ("cat", module_path!()),
                    ("[file]", file!()),
                    ("[line]", concat!(line!())),
                ]
            });
            // local_span_guard.add_property(|| ("[line]", line!().to_string().into()));
            Some(local_span_guard)
        };
    };
}

fn main() {
    let program_start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let collector = {
        let (root_span, collector) = Span::root("root");
        let _span_guard = root_span.set_local_parent();

        for _ in 0..1638400 {
            {
                enter_with_local_parent!("child");
            }
            {
                enter_with_local_parent!("child");
            }
            {
                enter_with_local_parent!("child 2");

                {
                    enter_with_local_parent!("child 3");
                }
            }
        }

        // do something ...
        collector
    };

    let spans = block_on(collector.collect());

    // println!("{:?}", spans);
    let mut trace_events = json::JsonValue::new_array();
    for span in spans {
        let mut event = Object::new();
        event.insert("ph", "X".into());
        event.insert("pid", Number::from(0).into());
        let start = span.begin_unix_time_ns - program_start;
        event.insert("ts", Number::from(start as f64 / 1e3).into());
        event.insert("dur", Number::from(span.duration_ns as f64 / 1e3).into());
        event.insert("name", span.event.into());
        let mut args = Object::new(); // file, line, get from properties
        for (k, v) in span.properties {
            if k == "cat" {
                event.insert("cat", v.to_string().into());
            } else {
                args.insert(k, v.to_string().into());
            }
        }
        for (k, v) in span.properties_static {
            if k == "cat" {
                event.insert("cat", v.to_string().into());
            } else {
                args.insert(k, v.to_string().into());
            }
        }
        event.insert("args", args.into());
        // entry.insert("cat", ""); // engine name
        // event.insert("tid", 0); // runtime id
        // entry.insert("args"); // file, line, get from properties
        // SAFETY: we not event and end_event are valid JsonValue
        trace_events.push(event).unwrap();
    }

    let mut events = json::JsonValue::new_object();
    events["displayTimeUnit"] = "ns".into();
    events["traceEvents"] = trace_events;
    // println!("{}", events.dump());
    fs::write("/tmp/a.json", events.dump()).unwrap();
}
