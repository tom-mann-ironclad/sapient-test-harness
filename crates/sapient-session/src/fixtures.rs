//! The default, self-consistent v2.0 `Registration` fixture and its
//! associated constants -- "what a valid test registration declares" for
//! this crate, used identically by its own test suite
//! (`tests/common/mod.rs` re-exports this module rather than defining its
//! own copy) and by the CLI's bundled default scenario suite
//! (Milestone 3), so there's exactly one definition rather than a third
//! copy alongside the two `dmm.rs`/`asm.rs` framing and fixture
//! duplications already eliminated once (see `framing.rs`'s module docs).

use sapient_conformance_core::bsi_flex_335_v2_0::{
    RangeBearing, RangeBearingCoordinateSystem, RangeBearingDatum, Registration,
    registration::{
        Capability, ClassDefinition, ConfigurationData, DetectionClassDefinition,
        DetectionDefinition, Duration, LocationType, ModeDefinition, ModeType, NodeDefinition,
        RegionDefinition, StatusDefinition, TaskDefinition, TimeUnits,
        location_type::{CoordinatesOneof, DatumOneof},
    },
};

pub const DEFAULT_MODE: &str = "Default";
pub const ALTERNATE_MODE: &str = "Alternate";
pub const DECLARED_CLASSIFICATION_TYPE: &str = "Human";
pub const STATUS_INTERVAL_SECONDS: f32 = 5.0;

/// Coordinate units shared by the default registration and its detection payload.
pub const DETECTION_COORDINATES: RangeBearingCoordinateSystem =
    RangeBearingCoordinateSystem::DegreesM;
/// North reference shared by the default registration and detection payload.
pub const DETECTION_DATUM: RangeBearingDatum = RangeBearingDatum::True;

/// A detection position consistent with both bundled modes' declared format.
pub fn detection_position() -> RangeBearing {
    RangeBearing {
        azimuth: Some(2.0),
        range: Some(100.0),
        coordinate_system: Some(DETECTION_COORDINATES as i32),
        datum: Some(DETECTION_DATUM as i32),
        ..Default::default()
    }
}

pub fn valid_location_type() -> LocationType {
    LocationType {
        coordinates_oneof: Some(CoordinatesOneof::RangeBearingUnits(
            DETECTION_COORDINATES as i32,
        )),
        datum_oneof: Some(DatumOneof::RangeBearingDatum(DETECTION_DATUM as i32)),
        zone: None,
    }
}

pub fn duration(units: TimeUnits, value: f32) -> Duration {
    Duration {
        units: Some(units as i32),
        value: Some(value),
    }
}

pub fn mode(name: &str, mode_type: ModeType) -> ModeDefinition {
    ModeDefinition {
        mode_name: Some(name.to_string()),
        mode_type: Some(mode_type as i32),
        mode_description: None,
        settle_time: Some(duration(TimeUnits::Seconds, 1.0)),
        maximum_latency: None,
        scan_type: None,
        tracking_type: None,
        duration: None,
        mode_parameter: vec![],
        detection_definition: vec![DetectionDefinition {
            behaviour_definition: vec![],
            detection_performance: vec![],
            detection_class_definition: vec![DetectionClassDefinition {
                confidence_definition: None,
                class_performance: vec![],
                class_definition: vec![ClassDefinition {
                    r#type: Some(DECLARED_CLASSIFICATION_TYPE.to_string()),
                    units: None,
                    sub_class: vec![],
                }],
                taxonomy_dock_definition: vec![],
            }],
            detection_report: vec![],
            geometric_error: None,
            velocity_type: None,
            location_type: Some(valid_location_type()),
        }],
        task: Some(TaskDefinition {
            command: vec![],
            concurrent_tasks: Some(1),
            region_definition: Some(RegionDefinition {
                settle_time: None,
                region_type: vec![1],
                region_area: vec![valid_location_type()],
                class_filter_definition: vec![],
                behaviour_filter_definition: vec![],
            }),
        }),
    }
}

/// A registration declaring two modes (one `MODE_TYPE_DEFAULT`, one
/// `MODE_TYPE_PERMANENT` reachable via a `mode_change` task -- named
/// [`DEFAULT_MODE`]/[`ALTERNATE_MODE`]) and a `status_interval` of
/// [`STATUS_INTERVAL_SECONDS`].
pub fn valid_registration() -> Registration {
    Registration {
        icd_version: Some("BSI Flex 335 v2.0".to_string()),
        node_definition: vec![NodeDefinition {
            node_type: Some(1),
            node_sub_type: vec![],
        }],
        name: None,
        short_name: None,
        capabilities: vec![Capability {
            category: Some("Radar".to_string()),
            r#type: Some("Range".to_string()),
            value: None,
            units: None,
        }],
        status_definition: Some(StatusDefinition {
            status_interval: Some(duration(TimeUnits::Seconds, STATUS_INTERVAL_SECONDS)),
            location_definition: None,
            coverage_definition: None,
            obscuration_definition: None,
            status_report: vec![],
            field_of_view_definition: None,
        }),
        mode_definition: vec![
            mode(DEFAULT_MODE, ModeType::Default),
            mode(ALTERNATE_MODE, ModeType::Permanent),
        ],
        reporting_region: vec![],
        dependent_nodes: vec![],
        config_data: vec![ConfigurationData {
            manufacturer: "Acme".to_string(),
            model: "Mk1".to_string(),
            serial_number: None,
            hardware_version: None,
            software_version: None,
            sub_components: vec![],
        }],
    }
}
