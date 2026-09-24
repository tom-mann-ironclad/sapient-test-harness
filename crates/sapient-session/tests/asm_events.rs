//! Progress events must identify real transitions, not just the current state.
mod common;

use common::{encode, envelope, valid_registration};
use sapient_conformance_core::bsi_flex_335_v2_0::{
    Alert, AlertAck, RegistrationAck, SapientMessage, sapient_message::Content,
};
use sapient_session::{AsmEvent, AsmSession, AsmSessionState};

const ASM: &str = "550e8400-e29b-41d4-a716-446655440000";
const DMM: &str = "550e8400-e29b-41d4-a716-446655440001";
const ALERT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const OTHER_ALERT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn feed(session: &mut AsmSession, content: Content) -> Option<Vec<u8>> {
    session.on_bytes(&encode(envelope(DMM, ASM, 1, content)))
}

fn registration_ack(acceptance: Option<bool>) -> Content {
    Content::RegistrationAck(RegistrationAck {
        acceptance,
        ack_response_reason: vec![],
    })
}

fn alert_ack(id: &str, status: i32) -> Content {
    Content::AlertAck(AlertAck {
        alert_id: Some(id.into()),
        alert_ack_status: Some(status),
        reason: vec![],
    })
}

fn registered_session() -> AsmSession {
    let mut session = AsmSession::new(ASM);
    session.register(valid_registration());
    feed(&mut session, registration_ack(Some(true)));
    assert_eq!(session.take_event(), Some(AsmEvent::RegistrationAccepted));
    session
}

#[test]
fn registration_outcomes_are_distinct_and_consumed_once() {
    for (acceptance, expected) in [
        (Some(true), AsmEvent::RegistrationAccepted),
        (Some(false), AsmEvent::RegistrationRejected),
        (None, AsmEvent::RegistrationFailed),
    ] {
        let mut session = AsmSession::new(ASM);
        assert_eq!(session.take_event(), None);
        session.register(valid_registration());
        assert_eq!(session.take_event(), None);
        feed(&mut session, registration_ack(acceptance));
        assert_eq!(session.take_event(), Some(expected));
        assert_eq!(session.take_event(), None);
    }
}

#[test]
fn acceptance_requires_a_usable_local_contract() {
    let mut session = AsmSession::new(ASM);
    let mut registration = valid_registration();
    registration.mode_definition.clear();
    session.register(registration);
    feed(&mut session, registration_ack(Some(true)));
    assert_eq!(session.take_event(), Some(AsmEvent::RegistrationFailed));
    assert!(matches!(session.state(), AsmSessionState::NotRegistered));
}

#[test]
fn unprompted_registration_ack_does_not_emit_acceptance() {
    let mut session = AsmSession::new(ASM);
    feed(&mut session, registration_ack(Some(true)));
    assert_eq!(session.take_event(), None);
    let mut session = registered_session();
    feed(&mut session, registration_ack(Some(true)));
    assert_eq!(session.take_event(), None);
}

#[test]
fn alert_progress_requires_validation_and_correlation() {
    let mut session = registered_session();
    for id in [ALERT, OTHER_ALERT] {
        session.issue_alert(Alert {
            alert_id: Some(id.into()),
            ..Default::default()
        });
    }
    // Correlated but malformed: must leave the alert outstanding.
    assert!(feed(&mut session, alert_ack(ALERT, 999)).is_some());
    assert_eq!(session.take_event(), None);
    feed(&mut session, alert_ack(OTHER_ALERT, 1));
    assert_eq!(
        session.take_event(),
        Some(AsmEvent::AlertAcknowledged {
            alert_id: OTHER_ALERT.into()
        })
    );
    feed(&mut session, alert_ack(ALERT, 1));
    assert_eq!(
        session.take_event(),
        Some(AsmEvent::AlertAcknowledged {
            alert_id: ALERT.into()
        })
    );
    assert_eq!(session.take_event(), None);
    // A duplicate acknowledgement is no longer correlated.
    feed(&mut session, alert_ack(ALERT, 1));
    assert_eq!(session.take_event(), None);
}

#[test]
fn unknown_and_pre_registration_alert_acks_do_not_emit_progress() {
    let mut session = AsmSession::new(ASM);
    feed(&mut session, alert_ack(ALERT, 1));
    assert_eq!(session.take_event(), None);
    let mut session = registered_session();
    feed(&mut session, alert_ack(ALERT, 1));
    assert_eq!(session.take_event(), None);
}

#[test]
fn next_frame_clears_unconsumed_events_even_on_decode_failure() {
    for raw in [vec![0xff], encode(SapientMessage::default())] {
        let mut session = registered_session();
        session.issue_alert(Alert {
            alert_id: Some(ALERT.into()),
            ..Default::default()
        });
        feed(&mut session, alert_ack(ALERT, 1));
        // Do not consume the valid event: the next frame must replace it.
        session.on_bytes(&raw);
        assert_eq!(session.take_event(), None);
        assert!(!session.findings().is_empty());
    }
}
