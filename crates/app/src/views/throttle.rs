//! Throttles: acquire trains by DCC address and drive them over the OpenLCB
//! Traction protocol.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use s4d_core::{LinkState, Throttle, ThrottleState};

use crate::bus::Bus;
use crate::platform;

/// Minimum time between SetSpeed commands from the slider (about 10 Hz).
const SPEED_INTERVAL_MS: u64 = 100;

/// Rate limiter for slider speed commands. The last value always gets sent.
#[derive(Default)]
struct SpeedLimiter {
    last_sent: u64,
    pending: Option<(f32, bool)>,
    scheduled: bool,
}

#[component]
pub fn ThrottleView() -> Element {
    let bus = use_context::<Bus>();
    let mut address = use_signal(|| "3".to_string());
    let mut long = use_signal(|| false);
    let (throttles, online) =
        bus.view(|d| (d.throttles.clone(), d.link_state() == LinkState::Online));

    // DCC short addresses stop at 127; anything above is a long address.
    let forced_long = address.read().trim().parse::<u16>().is_ok_and(|a| a > 127);
    let acquire = move || {
        if let Ok(a) = address.read().trim().parse::<u16>() {
            if (1..=10239).contains(&a) {
                let long = long() || a > 127;
                bus.act(|d, now| d.acquire(a, long, now));
            }
        }
    };

    rsx! {
        div { class: "throttle-view",
            form {
                class: "acquire",
                onsubmit: move |e| { e.prevent_default(); acquire(); },
                label { "DCC address"
                    input {
                        r#type: "number", inputmode: "numeric", min: "1", max: "10239",
                        value: "{address}",
                        oninput: move |e| address.set(e.value()),
                    }
                }
                label { class: "check",
                    input {
                        r#type: "checkbox",
                        checked: long() || forced_long,
                        disabled: forced_long,
                        title: if forced_long { "Addresses above 127 are always long" } else { "" },
                        onchange: move |e| long.set(e.checked()),
                    }
                    "Long address"
                }
                button { r#type: "submit", class: "primary", disabled: !online, "Acquire" }
                if !online { span { class: "muted", "Connect first." } }
            }
            if throttles.is_empty() {
                p { class: "muted center", "No throttles open. Enter a locomotive address and press Acquire." }
            }
            div { class: "throttles",
                for t in throttles.into_iter() {
                    ThrottleCard { key: "{t.id}", t }
                }
            }
        }
    }
}

#[component]
fn ThrottleCard(t: Throttle) -> Element {
    let bus = use_context::<Bus>();
    let id = t.id;
    let steps = (t.speed * 126.0).round() as i32;
    let active = t.state == ThrottleState::Active;
    let status = match &t.state {
        ThrottleState::Searching => "Searching…".to_string(),
        ThrottleState::Assigning => "Taking control…".to_string(),
        ThrottleState::Active => "In control".to_string(),
        ThrottleState::Failed(why) => why.to_string(),
    };
    let failed = matches!(t.state, ThrottleState::Failed(_));
    let reverse = t.reverse;
    let speed = t.speed;
    let functions = t.functions;
    let limiter = use_hook(|| Rc::new(RefCell::new(SpeedLimiter::default())));
    // Send at most ~10 SetSpeed per second while dragging; the final value
    // is sent when the interval ends. The displayed speed updates at once.
    let slide = move |v: f32, rev: bool| {
        let now = platform::now_ms();
        let mut l = limiter.borrow_mut();
        if !l.scheduled && now >= l.last_sent + SPEED_INTERVAL_MS {
            l.last_sent = now;
            drop(l);
            bus.act(|d, _| d.set_speed(id, v, rev));
            return;
        }
        l.pending = Some((v, rev));
        bus.act(|d, _| {
            if let Some(t) = d.throttles.iter_mut().find(|t| t.id == id) {
                t.speed = v;
                t.reverse = rev;
                d.revision += 1;
            }
        });
        if !l.scheduled {
            l.scheduled = true;
            let wait = (l.last_sent + SPEED_INTERVAL_MS).saturating_sub(now);
            let limiter = limiter.clone();
            spawn(async move {
                platform::sleep_ms(wait as u32).await;
                let next = {
                    let mut l = limiter.borrow_mut();
                    l.scheduled = false;
                    l.last_sent = platform::now_ms();
                    l.pending.take()
                };
                if let Some((v, rev)) = next {
                    bus.act(|d, _| d.set_speed(id, v, rev));
                }
            });
        }
    };

    rsx! {
        div { class: if failed { "card throttle failed" } else { "card throttle" },
            div { class: "card-head",
                span { class: "loco", "{t.address}" if t.long_address { small { " L" } } }
                span { class: "status", "{status}" }
                if failed {
                    button { onclick: move |_| bus.act(|d, now| d.retry(id, now)), "Retry" }
                }
                button { class: "ghost", title: "Release train", onclick: move |_| bus.act(|d, _| d.release(id)), "✕" }
            }
            div { class: "speed",
                div { class: "speed-readout", "{steps}" small { " / 126" } }
                input {
                    r#type: "range", min: "0", max: "126", step: "1",
                    value: "{steps}",
                    disabled: !active,
                    oninput: move |e| {
                        let v = e.value().parse::<f32>().unwrap_or(0.0) / 126.0;
                        slide(v, reverse);
                    },
                }
            }
            div { class: "dir-row",
                button {
                    class: if reverse { "on" } else { "" }, disabled: !active,
                    onclick: move |_| bus.act(|d, _| d.set_speed(id, speed, true)),
                    "◀ REV"
                }
                button {
                    disabled: !active,
                    onclick: move |_| bus.act(|d, _| d.set_speed(id, 0.0, reverse)),
                    "STOP"
                }
                button {
                    class: if !reverse { "on" } else { "" }, disabled: !active,
                    onclick: move |_| bus.act(|d, _| d.set_speed(id, speed, false)),
                    "FWD ▶"
                }
            }
            button { class: "estop", disabled: !active, onclick: move |_| bus.act(|d, _| d.emergency_stop(id)), "EMERGENCY STOP" }
            div { class: "functions",
                for f in 0..29u8 {
                    {
                        let on = functions[f as usize];
                        rsx! {
                            button {
                                class: if on { "fn on" } else { "fn" },
                                disabled: !active,
                                onclick: move |_| bus.act(|d, _| d.set_function(id, f, !on)),
                                if f == 0 { "F0 💡" } else { "F{f}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
