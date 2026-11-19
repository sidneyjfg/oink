//! Live check of the Linux backend against a real X session. Moves the pointer a few
//! pixels and scrolls one notch up and down. Never types and never clicks.
//! Run: DISPLAY=:1 XAUTHORITY=... cargo run -p oink-input --example linux_probe --release
use std::time::{Duration, Instant};

use oink_foundation::Point;
use oink_input::{InjectEvent, ScrollPhase, ScrollUnit};

fn stats(label: &str, mut v: Vec<Duration>) {
    v.sort();
    let n = v.len();
    let total: Duration = v.iter().sum();
    println!(
        "{label}: n={n} avg={:?} p50={:?} p99={:?} max={:?}",
        total / n as u32,
        v[n / 2],
        v[(n * 99 / 100).min(n - 1)],
        v[n - 1]
    );
}

fn main() {
    let backend = oink_input::platform_backend().expect("backend");
    let start = backend.cursor_position().expect("cursor_position");
    println!("posicao inicial: ({}, {})", start.x, start.y);

    // 1) smooth move 80px right and back, ~60 fps
    for i in 0..=40 {
        let t = f64::from(i) / 40.0;
        let eased = t * t * (3.0 - 2.0 * t);
        backend
            .inject(&InjectEvent::MoveTo {
                pos: Point::new(start.x + 80.0 * eased, start.y),
            })
            .expect("move");
        std::thread::sleep(Duration::from_millis(16));
    }
    for i in 0..=40 {
        let t = f64::from(i) / 40.0;
        let eased = t * t * (3.0 - 2.0 * t);
        backend
            .inject(&InjectEvent::MoveTo {
                pos: Point::new(start.x + 80.0 - 80.0 * eased, start.y),
            })
            .expect("move");
        std::thread::sleep(Duration::from_millis(16));
    }
    let back = backend.cursor_position().expect("cursor_position");
    println!("apos ida e volta: ({}, {})  [esperado igual ao inicial]", back.x, back.y);

    // 2) throughput: 2000 absolute moves on a circle, no sleeping
    let mut per_call = Vec::with_capacity(2000);
    let t0 = Instant::now();
    for i in 0..2000 {
        let a = f64::from(i) * 0.05;
        let c0 = Instant::now();
        backend
            .inject(&InjectEvent::MoveTo {
                pos: Point::new(start.x + 40.0 * a.cos(), start.y + 40.0 * a.sin()),
            })
            .expect("move");
        per_call.push(c0.elapsed());
    }
    let dt = t0.elapsed();
    println!("vazao: 2000 eventos em {dt:?} = {:.0} eventos/s", 2000.0 / dt.as_secs_f64());
    stats("custo de inject() (so envio)", per_call);

    // 3) round-trip: inject then read the pointer back (server has applied it)
    let mut rtt = Vec::with_capacity(300);
    let mut wrong = 0;
    for i in 0..300 {
        let target = Point::new(start.x + f64::from(i % 60), start.y + f64::from(i % 40));
        let c0 = Instant::now();
        backend.inject(&InjectEvent::MoveTo { pos: target }).expect("move");
        let seen = backend.cursor_position().expect("pos");
        rtt.push(c0.elapsed());
        if (seen.x - target.x).abs() > 0.5 || (seen.y - target.y).abs() > 0.5 {
            wrong += 1;
        }
    }
    stats("ida e volta (inject + ler posicao)", rtt);
    println!("posicoes divergentes do alvo: {wrong} de 300");

    // 4) relative motion and wheel
    backend.inject(&InjectEvent::MoveTo { pos: start }).expect("move");
    let before = backend.cursor_position().expect("pos");
    backend.inject(&InjectEvent::MoveRelative { dx: 25.0, dy: 10.0 }).expect("rel");
    let after = backend.cursor_position().expect("pos");
    println!(
        "relativo (+25,+10): ({}, {}) -> ({}, {})",
        before.x, before.y, after.x, after.y
    );
    backend
        .inject(&InjectEvent::Scroll {
            dx: 0.0,
            dy: 1.0,
            unit: ScrollUnit::Line,
            phase: ScrollPhase::None,
        })
        .expect("scroll up");
    std::thread::sleep(Duration::from_millis(250));
    backend
        .inject(&InjectEvent::Scroll {
            dx: 0.0,
            dy: -1.0,
            unit: ScrollUnit::Line,
            phase: ScrollPhase::None,
        })
        .expect("scroll down");

    // restore
    backend.inject(&InjectEvent::MoveTo { pos: start }).expect("restore");
    let end = backend.cursor_position().expect("pos");
    println!("posicao final restaurada: ({}, {})", end.x, end.y);
}
