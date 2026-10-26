use super::*;
use crate::layout::Feedback;
use crate::sim::Simulator;
use crate::testing::{Bench, BENCH_NODE_ID};

fn ev(n: u64) -> EventId {
    EventId(0x0501_0101_2299_0000 | n)
}

fn other_panel(id: u64) -> Node {
    Node::new(NodeId::new(0x0501_0101_3300 | id), NodeInfo::default())
}

fn status(position: Position) -> TurnoutStatus {
    TurnoutStatus {
        position,
        pending: None,
        stalled: false,
    }
}

/// Make the dispatcher's copy of T1 use feedback events nobody produces.
fn deaf_t1(b: &mut Bench) {
    let mut layout = b.d.layout().clone();
    let t = layout.turnouts.iter_mut().find(|t| t.id == "T1").unwrap();
    t.feedback = Some(Feedback {
        normal_event: Some(ev(1)),
        reverse_event: Some(ev(2)),
    });
    b.d.set_layout(layout);
}

#[test]
fn logs_in_and_learns_block_state() {
    let mut b = Bench::new();
    b.s.set_occupied("B2", true);
    assert_eq!(b.d.link_state(), LinkState::LoggingIn);
    b.run(500);
    assert_eq!(b.d.link_state(), LinkState::Online);
    assert_eq!(b.d.blocks["B2"], Occupancy::Occupied);
    assert_eq!(b.d.blocks["B1"], Occupancy::Clear);
    assert_eq!(b.d.turnouts["T1"].position, Position::Normal);

    b.s.set_occupied("B1", true);
    b.run(50);
    assert_eq!(b.d.blocks["B1"], Occupancy::Occupied);
}

#[test]
fn throw_with_and_without_feedback() {
    let mut b = Bench::new();
    b.run(500);
    assert!(b.d.throw("T1", Position::Reverse));
    assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Reverse));
    b.run(1000);
    assert_eq!(b.d.turnouts["T1"], status(Position::Reverse));
    assert_eq!(b.s.turnout_position("T1"), Some(Position::Reverse));

    // T3 has no feedback: optimistic.
    assert!(b.d.toggle("T3"));
    assert_eq!(b.d.turnouts["T3"].position, Position::Reverse);
    b.run(50);
    assert_eq!(b.s.turnout_position("T3"), Some(Position::Reverse));
}

#[test]
fn linked_turnouts_throw_together() {
    let mut b = Bench::new();
    let mut layout = b.d.layout().clone();
    layout.set_link("T1", Some("T2"), false);
    b.d.set_layout(layout.clone());
    b.replace_simulator(layout);
    b.run(500);
    assert!(b.d.throw("T2", Position::Reverse));
    b.run(1000);
    assert_eq!(b.s.turnout_position("T1"), Some(Position::Reverse));
    assert_eq!(b.s.turnout_position("T2"), Some(Position::Reverse));
    assert_eq!(b.d.turnouts["T1"].position, Position::Reverse);

    // Inverted link: the partner goes the other way.
    let mut layout = b.d.layout().clone();
    layout.set_link("T1", Some("T3"), true);
    b.d.set_layout(layout);
    b.d.throw("T1", Position::Normal);
    b.run(50);
    assert_eq!(b.s.turnout_position("T3"), Some(Position::Reverse));
    assert_eq!(
        b.s.turnout_position("T2"),
        Some(Position::Reverse),
        "T2 no longer linked"
    );
}

/// Regression: Producer Identified for a command event (as announced by
/// every other panel at login) used to set a `pending` that never cleared.
#[test]
fn producer_identified_on_command_is_not_an_action() {
    let mut b = Bench::new();
    b.run(500);
    let layout = b.d.layout().clone();
    let t1 = layout.turnout("T1").unwrap().clone();
    let t3 = layout.turnout("T3").unwrap().clone();
    assert!(t1.has_feedback() && !t3.has_feedback());

    let mut panel = other_panel(1);
    panel.add_producer(t1.reverse_event.unwrap(), EventState::Valid);
    panel.add_producer(t1.normal_event.unwrap(), EventState::Invalid);
    panel.add_producer(t3.reverse_event.unwrap(), EventState::Valid);
    panel.add_producer(t3.normal_event.unwrap(), EventState::Invalid);
    b.add_node(panel);
    b.run(500);

    // T1 has feedback: the announcement changes nothing.
    assert_eq!(b.d.turnouts["T1"], status(Position::Normal));
    // T3 has none: the other panel's last command is our best knowledge.
    assert_eq!(b.d.turnouts["T3"], status(Position::Reverse));
    b.run(PENDING_TIMEOUT_MS + 100);
    assert_eq!(b.d.turnouts["T1"], status(Position::Normal));
}

#[test]
fn turnout_with_silent_feedback_learns_position_from_its_driver() {
    let mut b = Bench::new();
    deaf_t1(&mut b);
    b.run(500);
    // Nobody answers for the feedback events, but the simulated driver
    // reports its output as Consumer Identified Valid.
    assert_eq!(b.d.turnouts["T1"], status(Position::Normal));
}

#[test]
fn unknown_state_is_queried_again() {
    let mut b = Bench::new();
    let mut layout = b.d.layout().clone();
    let b1 = layout.blocks.iter_mut().find(|x| x.id == "B1").unwrap();
    b1.occupied_event = Some(ev(10));
    b1.clear_event = Some(ev(11));
    b.d.set_layout(layout);
    // A detector that doesn't know its state yet when we first ask.
    let mut det = other_panel(3);
    det.add_producer(ev(10), EventState::Unknown);
    det.add_producer(ev(11), EventState::Unknown);
    let i = b.add_node(det);
    b.run(500);
    assert_eq!(b.d.link_state(), LinkState::Online);
    assert_eq!(b.d.blocks["B1"], Occupancy::Unknown);

    // It finds out, but sensors only report changes...
    b.nodes[i].set_producer_state(ev(10), EventState::Valid);
    b.nodes[i].set_producer_state(ev(11), EventState::Invalid);
    b.run(500);
    assert_eq!(b.d.blocks["B1"], Occupancy::Unknown);
    // ...so the dispatcher asks again.
    b.run(REQUERY_MS);
    assert_eq!(b.d.blocks["B1"], Occupancy::Occupied);
}

#[test]
fn requery_stops_when_everything_is_known() {
    let mut b = Bench::new();
    b.run(500);
    b.run(REQUERY_MS + 100);
    assert_eq!(b.d.requery, None);
    assert_eq!(b.d.next_deadline(), None);
}

#[test]
fn pcer_from_another_panel_sets_pending_until_feedback() {
    let mut b = Bench::new();
    b.run(500);
    let t1 = b.d.layout().turnout("T1").unwrap().clone();
    let i = b.add_node(other_panel(2));
    b.run(300);
    b.nodes[i].send(Message::Pcer(t1.reverse_event.unwrap()));
    b.settle();
    assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Reverse));
    b.run(1000);
    assert_eq!(b.d.turnouts["T1"], status(Position::Reverse));
}

/// Regression: commanding the position a turnout already has left it
/// pending forever, since sensors only report changes.
#[test]
fn throw_to_current_position_is_confirmed_by_query() {
    let mut b = Bench::new();
    b.run(500);
    assert_eq!(b.s.turnout_position("T1"), Some(Position::Normal));
    assert!(b.d.throw("T1", Position::Normal));
    assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Normal));
    // Well before the simulator's 400 ms feedback delay.
    b.run(50);
    assert_eq!(b.d.turnouts["T1"], status(Position::Normal));
    assert!(b.d.pending_deadlines.is_empty());
}

#[test]
fn unconfirmed_throw_stalls_after_deadline() {
    let mut b = Bench::new();
    deaf_t1(&mut b);
    b.run(500);
    assert!(b.d.throw("T1", Position::Reverse));
    let deadline = b.now + PENDING_TIMEOUT_MS;
    assert!(b.d.next_deadline().unwrap() <= deadline);
    b.run(PENDING_TIMEOUT_MS - 100);
    assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Reverse));
    assert!(!b.d.turnouts["T1"].stalled);
    b.run(200);
    let st = b.d.turnouts["T1"];
    assert_eq!(st.pending, None);
    assert!(st.stalled);

    // The next real feedback clears it.
    let i = b.add_node(other_panel(3));
    b.nodes[i].add_producer(ev(2), EventState::Valid);
    b.run(300);
    b.nodes[i].send(Message::Pcer(ev(2)));
    b.settle();
    assert_eq!(b.d.turnouts["T1"], status(Position::Reverse));
}

#[test]
fn block_consumer_states_follow_occupancy() {
    let mut b = Bench::new();
    b.s.set_occupied("B2", true);
    b.run(500);
    let blk = b.d.layout().block("B2").unwrap().clone();
    let i = b.add_node(other_panel(4));
    b.run(300);
    b.inbox[i].clear();
    b.nodes[i].send(Message::IdentifyConsumer(blk.occupied_event.unwrap()));
    b.nodes[i].send(Message::IdentifyConsumer(blk.clear_event.unwrap()));
    b.settle();
    let states: Vec<_> = b.inbox[i]
        .iter()
        .filter_map(|r| match r.msg {
            Message::ConsumerIdentified { event, state } => Some((event, state)),
            _ => None,
        })
        .collect();
    assert!(states.contains(&(blk.occupied_event.unwrap(), EventState::Valid)));
    assert!(states.contains(&(blk.clear_event.unwrap(), EventState::Invalid)));

    b.d.link_down();
    assert!(b.d.blocks.values().all(|o| *o == Occupancy::Unknown));
}

#[test]
fn throttle_acquire_drive_release() {
    let mut b = Bench::new();
    b.run(500);
    let id = b.d.acquire(1234, false, b.now);
    assert_eq!(b.d.throttles[0].state, ThrottleState::Searching);
    assert!(b.d.throttles[0].long_address, "above 127 is always long");
    b.run(600);
    assert_eq!(b.d.throttles[0].state, ThrottleState::Active);

    b.d.set_speed(id, 0.5, true);
    b.d.set_function(id, 0, true);
    b.run(50);
    let train = b.s.train(1234).unwrap();
    assert!(train.speed.is_reverse());
    assert!((train.speed.mph() - 63.0).abs() < 0.5);
    assert!(train.functions[0]);
    assert!(train.controller.is_some());

    b.d.release(id);
    b.run(50);
    assert!(b.d.throttles.is_empty());
    assert!(b.s.train(1234).unwrap().controller.is_none());
}

/// Regression: acquiring the same train twice opened two throttles; and
/// short 3 and long 3 were the same simulated train.
#[test]
fn acquire_reuses_throttle_and_tells_long_from_short() {
    let mut b = Bench::new();
    b.run(500);
    let a = b.d.acquire(3, false, b.now);
    assert_eq!(b.d.acquire(3, false, b.now), a);
    let long = b.d.acquire(3, true, b.now);
    assert_ne!(long, a);
    assert_eq!(b.d.throttles.len(), 2);
    b.run(600);
    assert!(b
        .d
        .throttles
        .iter()
        .all(|t| t.state == ThrottleState::Active));
    assert_ne!(b.d.throttles[0].train_alias, b.d.throttles[1].train_alias);
    let short_train = b.s.find_train(3, false).unwrap();
    let long_train = b.s.find_train(3, true).unwrap();
    assert_ne!(short_train.node.node_id(), long_train.node.node_id());
    assert!(short_train.controller.is_some() && long_train.controller.is_some());
    assert_eq!(b.s.trains().len(), 2);
    assert_eq!(b.d.acquire(3, true, b.now), long);
}

#[test]
fn release_keeps_train_shared_with_another_throttle() {
    let mut b = Bench::new();
    b.run(500);
    let first = b.d.acquire(5, false, b.now);
    b.run(600);
    // Pretend the first throttle failed, so a second one may be opened...
    b.d.throttles[0].state = ThrottleState::Failed(FailReason::NoResponse);
    let second = b.d.acquire(5, false, b.now);
    assert_ne!(first, second);
    b.run(600);
    // ...and then the first is retried onto the same train.
    b.d.retry(first, b.now);
    b.run(600);
    assert!(b
        .d
        .throttles
        .iter()
        .all(|t| t.state == ThrottleState::Active));

    b.d.release(first);
    b.run(50);
    assert!(b.s.train(5).unwrap().controller.is_some());
    b.d.release(second);
    b.run(50);
    assert!(b.s.train(5).unwrap().controller.is_none());
}

/// Have the command station create (and log in) a train, then let it go.
fn warm_train(b: &mut Bench, address: u16) {
    let id = b.d.acquire(address, false, b.now);
    b.run(600);
    b.d.release(id);
    b.run(50);
    assert!(b.s.train(address).unwrap().controller.is_none());
}

/// Regression: releasing while Assigning left the train bound to us.
#[test]
fn release_while_assigning_releases_train() {
    let mut b = Bench::new();
    b.run(500);
    warm_train(&mut b, 7);
    let id = b.d.acquire(7, false, b.now);
    // Deliver the search and its reply, so the assign command is queued.
    let search = b.d.take_outgoing();
    for f in &search {
        b.s.receive(f, b.now);
    }
    for f in &b.s.take_outgoing() {
        b.d.receive(f, b.now);
    }
    assert_eq!(b.d.throttles[0].state, ThrottleState::Assigning);
    b.d.release(id);
    b.run(100);
    assert!(b.d.throttles.is_empty());
    assert!(b.s.train(7).unwrap().controller.is_none());
}

/// Regression: an assignment accepted after we gave up kept the train
/// bound to us.
#[test]
fn late_assign_reply_is_released() {
    let mut b = Bench::new();
    b.run(500);
    warm_train(&mut b, 8);
    b.d.acquire(8, false, b.now);
    for f in &b.d.take_outgoing() {
        b.s.receive(f, b.now);
    }
    for f in &b.s.take_outgoing() {
        b.d.receive(f, b.now);
    }
    // The assign command is queued but the train hasn't seen it yet.
    b.now += ASSIGN_TIMEOUT_MS + 10;
    b.d.poll(b.now);
    assert_eq!(
        b.d.throttles[0].state,
        ThrottleState::Failed(FailReason::NoResponse)
    );
    b.run(100);
    assert!(b.s.train(8).unwrap().controller.is_none());
    // Closing the failed throttle still sends a release; harmless.
    let id = b.d.throttles[0].id;
    b.d.release(id);
    b.run(50);
    assert!(b.d.throttles.is_empty());
}

#[test]
fn controller_changed_fails_throttle() {
    let mut b = Bench::new();
    b.run(500);
    b.d.acquire(9, false, b.now);
    b.run(600);
    assert_eq!(b.d.throttles[0].state, ThrottleState::Active);
    let me = b.d.alias().unwrap();
    let train = b.s.train_mut(9, false).unwrap();
    train.node.send(Message::TractionReply {
        dest: me,
        payload: Reply::ControllerChanged { result: 0 }.encode(),
    });
    b.run(50);
    let t = &b.d.throttles[0];
    assert_eq!(t.state, ThrottleState::Failed(FailReason::TakenOver));
    assert_eq!(t.train_alias, None);
    assert_eq!(
        FailReason::TakenOver.to_string(),
        "taken over by another throttle"
    );
}

#[test]
fn heartbeat_is_answered() {
    let mut b = Bench::new();
    b.run(500);
    b.d.acquire(3, false, b.now);
    b.run(600);
    let before = b.s.train(3).unwrap().noops;
    b.run(crate::sim::HEARTBEAT_MS + 100);
    assert!(b.s.train(3).unwrap().noops > before);
}

#[test]
fn link_down_resets() {
    let mut b = Bench::new();
    b.run(500);
    b.d.acquire(3, false, b.now);
    b.run(600);
    b.d.link_down();
    assert_eq!(b.d.link_state(), LinkState::Offline);
    assert_eq!(b.d.blocks["B1"], Occupancy::Unknown);
    assert_eq!(
        b.d.throttles[0].state,
        ThrottleState::Failed(FailReason::ConnectionLost)
    );
    assert!(!b.d.throw("T1", Position::Reverse));
}

/// Regression: replacing the simulator without link_down left throttles
/// "Active" against aliases that no longer exist.
#[test]
fn link_up_fails_running_throttles() {
    let mut b = Bench::new();
    b.run(500);
    b.d.acquire(3, false, b.now);
    b.run(600);
    assert_eq!(b.d.throttles[0].state, ThrottleState::Active);
    let layout = b.d.layout().clone();
    b.replace_simulator(layout);
    b.d.link_up(b.now);
    assert_eq!(
        b.d.throttles[0].state,
        ThrottleState::Failed(FailReason::ConnectionLost)
    );
    assert_eq!(b.d.throttles[0].train_alias, None);
    // And they can be retried against the new bus.
    b.run(500);
    let id = b.d.throttles[0].id;
    b.d.retry(id, b.now);
    b.run(600);
    assert_eq!(b.d.throttles[0].state, ThrottleState::Active);
}

#[test]
fn search_times_out() {
    let mut d = Dispatcher::new(BENCH_NODE_ID, Layout::demo());
    d.link_up(0);
    d.poll(300);
    d.acquire(99, false, 300);
    d.poll(300 + SEARCH_TIMEOUT_MS);
    assert_eq!(
        d.throttles[0].state,
        ThrottleState::Failed(FailReason::NoTrain { address: 99 })
    );
}

#[test]
fn acquire_offline_fails_not_connected() {
    let mut d = Dispatcher::new(BENCH_NODE_ID, Layout::demo());
    d.acquire(3, false, 0);
    assert_eq!(
        d.throttles[0].state,
        ThrottleState::Failed(FailReason::NotConnected)
    );
}

#[test]
fn fail_reasons_display() {
    let all = [
        FailReason::NotConnected,
        FailReason::NoTrain { address: 3 },
        FailReason::NoResponse,
        FailReason::Refused,
        FailReason::Rejected(0x1000),
        FailReason::ConnectionLost,
        FailReason::TakenOver,
    ];
    for r in all {
        assert!(!r.to_string().is_empty());
    }
    assert_eq!(
        FailReason::NoTrain { address: 3 }.to_string(),
        "no train found for address 3"
    );
    assert!(FailReason::Rejected(0x1000).to_string().contains("1000"));
}

#[test]
fn hardware_version_is_configurable() {
    let mut info = Dispatcher::default_info();
    info.hardware_version = "Desktop".into();
    let mut b = Bench::new();
    b.d = Dispatcher::with_info(BENCH_NODE_ID, Layout::demo(), info);
    b.d.link_up(b.now);
    b.run(500);
    let i = b.add_node(other_panel(5));
    b.run(300);
    let dest = b.d.alias().unwrap();
    b.nodes[i].send(Message::SnipRequest { dest });
    b.run(50);
    let snip = b.inbox[i]
        .iter()
        .find_map(|r| match &r.msg {
            Message::SnipReply { data, .. } => Some(NodeInfo::from_snip(data)),
            _ => None,
        })
        .expect("SNIP reply");
    assert_eq!(snip.hardware_version, "Desktop");
    assert_eq!(snip.model, "System4Dispatch");
}

#[test]
fn simulator_is_a_fresh_bus_after_replacement() {
    // Sanity check for the bench itself.
    let mut b = Bench::new();
    b.run(500);
    b.s = Simulator::new(Layout::demo(), b.now);
    b.s.random_occupancy = false;
    b.d.link_down();
    b.d.link_up(b.now);
    b.run(500);
    assert_eq!(b.d.link_state(), LinkState::Online);
    assert_eq!(b.d.blocks["B1"], Occupancy::Clear);
}

#[test]
fn controller_changing_notify_is_acknowledged_and_releases_the_throttle() {
    let mut b = Bench::new();
    b.run(500);
    b.d.acquire(3, false, b.now);
    b.run(600);
    assert_eq!(b.d.throttles[0].state, ThrottleState::Active);
    let train = b.d.throttles[0].train_alias.unwrap();
    let me = b.d.alias().unwrap();

    // The train announces that another controller is taking over.
    let notify = Message::TractionCommand {
        dest: me,
        payload: Command::ControllerChangingNotify {
            flags: 0,
            new_controller: NodeId::new(0x0501_0101_9999),
        }
        .encode(),
    };
    for f in notify.to_frames(train) {
        b.d.receive(&f, b.now);
    }
    assert_eq!(
        b.d.throttles[0].state,
        ThrottleState::Failed(FailReason::TakenOver)
    );
    assert_eq!(b.d.throttles[0].train_alias, None);

    // We acknowledged it with a Controller Changed reply, not a rejection.
    let mut asm = rolcb::message::Assembler::new();
    let replies: Vec<_> =
        b.d.take_outgoing()
            .iter()
            .filter_map(|f| asm.accept(f))
            .map(Message::from)
            .collect();
    assert!(replies.iter().any(|m| matches!(m,
        Message::TractionReply { dest, payload } if *dest == train && Reply::decode(payload) == Reply::ControllerChanged { result: 0 })),
        "{replies:?}");
}

fn table(b: &Bench) -> TurntableStatus {
    b.d.turntables["TT1"]
}

fn status_at(position: usize) -> TurntableStatus {
    TurntableStatus {
        position: Some(position),
        pending: None,
        stalled: false,
    }
}

#[test]
fn turntable_steps_and_confirms() {
    let mut b = Bench::new(); // demo TT1: every track has feedback
    b.run(500);
    // The simulated bridge starts at the first track, learned from feedback.
    assert_eq!(table(&b), status_at(0));
    assert!(b.d.step_turntable("TT1", true));
    assert_eq!(table(&b).pending, Some(1));
    b.run(1_000);
    assert_eq!(table(&b), status_at(1));
    // Counter-clockwise past the start wraps round to the last track (the lead).
    assert!(b.d.step_turntable("TT1", false));
    assert!(b.d.step_turntable("TT1", false));
    assert_eq!(table(&b).pending, Some(7));
    b.run(2_000);
    assert_eq!(table(&b), status_at(7));
    assert_eq!(b.s.turntable_position("TT1"), Some(7));
}

#[test]
fn turn_to_takes_the_short_way() {
    let mut b = Bench::new();
    b.run(500);
    let n = b.d.layout().turntables[0].tracks.len();
    // From track 0 to track 6 of 8 is two steps counter-clockwise.
    assert!(b.d.turn_to("TT1", 6));
    assert_eq!(table(&b).pending, Some(6));
    b.run(600);
    // Passing track 7 on the way: still turning.
    assert_eq!(table(&b).position, Some(n - 1));
    assert_eq!(table(&b).pending, Some(6));
    b.run(1_000);
    assert_eq!(table(&b), status_at(6));
    assert_eq!(b.s.turntable_position("TT1"), Some(6));
    // Already there: nothing to send.
    assert!(b.d.turn_to("TT1", 6));
    assert_eq!(table(&b), status_at(6));
    assert!(!b.d.turn_to("TT1", 99));
}

#[test]
fn turntable_without_feedback_needs_its_position_set() {
    let mut layout = Layout::demo();
    for tr in &mut layout.turntables[0].tracks {
        tr.feedback_event = None;
    }
    let mut b = Bench::with_layout(layout);
    b.run(500);
    assert_eq!(table(&b).position, None);
    // Without a known position, only single steps can be sent.
    assert!(!b.d.turn_to("TT1", 3));
    assert!(b.d.step_turntable("TT1", true));
    assert_eq!(table(&b).position, None);
    b.run(1_000);
    assert_eq!(b.s.turntable_position("TT1"), Some(1));
    b.d.set_turntable_position("TT1", Some(1));
    assert!(b.d.turn_to("TT1", 3));
    assert_eq!(table(&b), status_at(3));
    b.run(2_000);
    assert_eq!(b.s.turntable_position("TT1"), Some(3));
    // Offline: refused, and the position is forgotten.
    b.d.link_down();
    assert!(!b.d.step_turntable("TT1", true));
    assert_eq!(table(&b), TurntableStatus::default());
}

#[test]
fn turntable_stepped_by_someone_else_is_followed() {
    let mut b = Bench::new();
    let n = b.add_node(other_panel(1));
    b.run(500);
    let cw = b.d.layout().turntables[0].cw_event.unwrap();
    b.nodes[n].send(Message::Pcer(cw));
    b.run(20);
    assert_eq!(table(&b).pending, Some(1));
    b.run(1_000);
    assert_eq!(table(&b), status_at(1));
}

#[test]
fn turntable_without_confirmation_stalls() {
    let mut b = Bench::new();
    b.run(500);
    let mut layout = b.d.layout().clone();
    for (i, tr) in layout.turntables[0].tracks.iter_mut().enumerate() {
        tr.feedback_event = Some(ev(0x100 + i as u64));
    }
    b.d.set_layout(layout);
    b.d.set_turntable_position("TT1", Some(0));
    assert!(b.d.step_turntable("TT1", true));
    b.run(TURNTABLE_TIMEOUT_MS - 1_000);
    assert_eq!(table(&b).pending, Some(1));
    b.run(2_000);
    assert_eq!(table(&b).pending, None);
    assert!(table(&b).stalled);
}

#[test]
fn switches_with_two_turnouts_step_through_routes() {
    use crate::Shape;
    let mut b = Bench::new();
    b.run(500);
    // Any two turnouts will do: T1 has feedback, T3 doesn't.
    let n = Position::Normal;
    let r = Position::Reverse;
    assert_eq!(b.d.pair_positions("T1", "T3"), (n, n));
    // 3-way: straight -> left -> right -> straight.
    assert_eq!(b.d.next_route(Shape::ThreeWay, "T1", "T3"), Some((r, n)));
    b.run(1_000);
    assert_eq!(b.s.turnout_position("T1"), Some(r));
    assert_eq!(b.d.next_route(Shape::ThreeWay, "T1", "T3"), Some((n, r)));
    b.run(1_000);
    assert_eq!(b.s.turnout_position("T1"), Some(n));
    assert_eq!(b.s.turnout_position("T3"), Some(r));
    assert_eq!(b.d.next_route(Shape::ThreeWay, "T1", "T3"), Some((n, n)));
    b.run(1_000);
    // Double slip: four routes, ending with both reversed.
    for want in [(r, n), (n, r), (r, r), (n, n)] {
        assert_eq!(b.d.next_route(Shape::DoubleSlip, "T1", "T3"), Some(want));
        b.run(1_000);
        assert_eq!(b.d.pair_positions("T1", "T3"), want);
    }
    // A position combination that isn't a route starts again at the first.
    assert!(b.d.set_pair("T1", "T3", (n, r)));
    b.run(1_000);
    assert_eq!(b.d.next_route(Shape::SingleSlip, "T1", "T3"), Some((n, n)));
}

// ---- Node configuration ---------------------------------------------------

const SIM_NODE: NodeId = NodeId::new(0x0501_0101_FF00);

fn sim_cdi(b: &Bench) -> crate::cdi::Cdi {
    match &b.d.config.data[&SIM_NODE].cdi {
        Some(Ok(c)) => c.clone(),
        other => panic!("CDI not loaded: {other:?}"),
    }
}

fn field<'a>(cdi: &'a crate::cdi::Cdi, name: &str) -> &'a crate::cdi::Field {
    cdi.fields().into_iter().find(|f| f.name == name).unwrap()
}

#[test]
fn discovers_nodes_with_their_names() {
    let mut b = Bench::new();
    b.run(500);
    let n = &b.d.config.nodes[&SIM_NODE];
    assert_eq!(n.info.as_ref().unwrap().model, "Simulated layout");
    assert!(n.has_cdi());
    assert_eq!(
        n.protocols.unwrap() & rolcb::message::protocol::MEMORY_CONFIGURATION,
        0x10_0000
    );
}

#[test]
fn reads_cdi_and_values_and_writes_them() {
    let mut b = Bench::new();
    b.run(500);
    b.d.load_cdi(SIM_NODE);
    assert_eq!(b.d.config.busy(SIM_NODE), 1);
    b.run(2_000);
    assert_eq!(b.d.config.busy(SIM_NODE), 0);
    let cdi = sim_cdi(&b);
    assert_eq!(cdi.identification.model, "Simulated layout");
    // <acdi/> brings the user name and description.
    assert_eq!(cdi.segments[0].space, crate::cdi::SPACE_ACDI_USER);

    b.d.read_all_config(SIM_NODE);
    b.run(2_000);
    let data = &b.d.config.data[&SIM_NODE];
    assert_eq!(data.error, None);
    let delay = field(&cdi, "Turnout feedback delay (ms)");
    let bytes = data.bytes(delay.space, delay.address, delay.size).unwrap();
    assert_eq!(delay.decode(&bytes).unwrap(), "400");
    // The bench turned random occupancy off.
    let random = field(&cdi, "Random occupancy");
    let bytes = data
        .bytes(random.space, random.address, random.size)
        .unwrap();
    assert_eq!(random.decode(&bytes).unwrap(), "0");

    // Rename the node: SNIP follows.
    let name = field(&cdi, "Name");
    b.d.write_config(
        SIM_NODE,
        name.space,
        name.address,
        &name.encode("Hilltown IO").unwrap(),
    );
    b.d.config_update_complete(SIM_NODE);
    b.run(1_000);
    assert_eq!(b.d.config.data[&SIM_NODE].error, None);
    assert_eq!(b.d.config.busy_total(), 0);
    // The new name is fetched after the write.
    assert_eq!(b.d.config.nodes[&SIM_NODE].label(SIM_NODE), "Hilltown IO");

    // A live setting: a slower turnout.
    b.d.write_config(
        SIM_NODE,
        delay.space,
        delay.address,
        &delay.encode("2000").unwrap(),
    );
    b.run(500);
    assert!(b.d.throw("T1", Position::Reverse));
    b.run(1_000);
    assert_eq!(b.d.turnouts["T1"].pending, Some(Position::Reverse));
    b.run(1_500);
    assert_eq!(b.d.turnouts["T1"], status(Position::Reverse));
}

#[test]
fn node_without_memory_config_reports_an_error() {
    let mut b = Bench::new();
    let i = b.add_node(other_panel(4));
    b.run(500);
    let id = b.nodes[i].node_id();
    assert!(b.d.config.nodes.contains_key(&id));
    b.d.load_cdi(id);
    b.run(500);
    let err = b.d.config.data[&id].error.clone().unwrap();
    assert!(err.contains("1043"), "{err}");
    assert_eq!(b.d.config.busy_total(), 0);
}

#[test]
fn silent_node_times_out() {
    let mut b = Bench::new();
    b.run(500);
    // A node we know of that has left the bus without saying so.
    let gone = NodeId::new(0x0501_0101_7700);
    b.d.config.saw_node(gone, 0x777);
    b.d.load_cdi(gone);
    b.run(crate::config::ACK_TIMEOUT_MS + 100);
    assert!(b.d.config.data[&gone]
        .error
        .as_ref()
        .unwrap()
        .contains("acknowledge"));
    assert_eq!(b.d.config.busy_total(), 0);
}

#[test]
fn datagrams_other_than_replies_are_rejected() {
    let mut b = Bench::new();
    let i = b.add_node(other_panel(5));
    b.run(500);
    let me = b.d.alias().unwrap();
    let them = b.nodes[i].alias().unwrap();
    for (data, code) in [
        (vec![0x30, 1, 2], 0x1042),
        (vec![0x20, 0x43, 0, 0, 0, 0, 64], 0x1041),
    ] {
        b.inbox[i].clear();
        b.nodes[i].send(Message::Datagram { dest: me, data });
        b.settle();
        assert_eq!(
            b.inbox[i]
                .iter()
                .filter(|r| r.msg.dest() == Some(them))
                .map(|r| r.msg.clone())
                .collect::<Vec<_>>(),
            vec![Message::DatagramRejected { dest: them, code }]
        );
    }
}

#[test]
fn nodes_are_discovered_once_per_login() {
    let mut b = Bench::new();
    let i = b.add_node(other_panel(6));
    b.run(500);
    let verifies = |b: &Bench| {
        b.inbox[i]
            .iter()
            .filter(|r| {
                matches!(
                    r.msg,
                    Message::VerifyNodeIdGlobal { .. } | Message::SnipRequest { .. }
                )
            })
            .count()
    };
    let after_login = verifies(&b);
    assert!(after_login > 0);
    // Applying a layout queries state again, but doesn't rediscover.
    let layout = b.d.layout().clone();
    b.d.set_layout(layout);
    b.run(500);
    assert_eq!(verifies(&b), after_login);
}
