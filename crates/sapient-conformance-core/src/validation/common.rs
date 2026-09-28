use ulid::Ulid;
use uuid::Uuid;

use crate::bsi_flex_335_v2_0::{
    AssociatedDetection, AssociatedFile, FollowObject, Location, LocationList,
    LocationOrRangeBearing, RangeBearing, RangeBearingCone,
};
use crate::finding::ValidationOutcome;
use prost_types::Timestamp;

pub fn validate_ulid(
    id: Option<&str>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    let rule_id = rule_id.into();
    let value = match id {
        Some(value) => value,
        None => return ValidationOutcome::fail(rule_id, error_message),
    };

    let parsed = match Ulid::from_string(value) {
        Ok(parsed) => parsed,
        Err(_) => return ValidationOutcome::fail(rule_id, error_message),
    };

    if parsed.to_string() != value {
        return ValidationOutcome::fail(rule_id, error_message);
    }

    ValidationOutcome::pass()
}

pub fn validate_uuid_v4(
    id: Option<&str>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    let rule_id = rule_id.into();
    let value = match id {
        Some(value) => value,
        None => return ValidationOutcome::fail(rule_id, error_message),
    };

    let parsed = Uuid::parse_str(value);

    match parsed {
        Ok(uuid)
            if uuid.get_version_num() == 4
                && uuid.get_variant() == uuid::Variant::RFC4122
                && uuid.hyphenated().to_string() == value =>
        {
            ValidationOutcome::pass()
        }
        _ => ValidationOutcome::fail(rule_id, error_message),
    }
}

/// Earliest `seconds` a protobuf `Timestamp` can represent: `0001-01-01T00:00:00Z`,
/// per the well-known type's own documented range.
const MIN_TIMESTAMP_SECONDS: i64 = -62_135_596_800;
/// Latest `seconds` a protobuf `Timestamp` can represent: `9999-12-31T23:59:59Z`.
const MAX_TIMESTAMP_SECONDS: i64 = 253_402_300_799;

pub fn validate_timestamp(
    timestamp: Option<Timestamp>,
    missing_rule_id: impl Into<String>,
    missing_error_message: &str,
    malformed_rule_id: impl Into<String>,
    malformed_error_message: &str,
) -> ValidationOutcome {
    let timestamp = match timestamp {
        Some(timestamp) => timestamp,
        None => return ValidationOutcome::fail(missing_rule_id, missing_error_message),
    };

    if !(0..1_000_000_000).contains(&timestamp.nanos)
        || !(MIN_TIMESTAMP_SECONDS..=MAX_TIMESTAMP_SECONDS).contains(&timestamp.seconds)
    {
        return ValidationOutcome::fail(malformed_rule_id, malformed_error_message);
    }

    ValidationOutcome::pass()
}

pub fn validate_unit_interval(
    value: Option<f32>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(value) if (0.0..=1.0).contains(&value) => ValidationOutcome::pass(),
        Some(_) => ValidationOutcome::fail(rule_id, error_message),
        None => ValidationOutcome::pass(),
    }
}

pub fn validate_required_string(
    value: Option<&str>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some("") | None => ValidationOutcome::fail(rule_id, error_message),
        Some(_) => ValidationOutcome::pass(),
    }
}

pub fn validate_required_nonzero(
    value: Option<i32>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        None | Some(0) => ValidationOutcome::fail(rule_id, error_message),
        Some(_) => ValidationOutcome::pass(),
    }
}

pub fn validate_nonzero(
    value: i32,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        0 => ValidationOutcome::fail(rule_id, error_message),
        _ => ValidationOutcome::pass(),
    }
}

/// Membership check for a mandatory enum field whose v2.0-defined
/// discriminants are exactly the contiguous range `1..=max` (0 is
/// `..._UNSPECIFIED` and, like absence, invalid for a mandatory field).
/// Prefer this over [`validate_required_nonzero`] for any protocol enum --
/// nonzero alone lets an undefined positive discriminant (e.g. `999`) pass,
/// and protobuf's wire format does not drop or reject those: unknown
/// enumeration values decode and survive as their raw `i32` untouched.
/// Only use this where the enum's defined values really are gap-free --
/// an enum with a `reserved` (retired) value in the middle of its range
/// needs its own explicit membership list instead (see
/// [`validate_location_coordinate_system`] for an example).
pub fn validate_required_enum(
    value: Option<i32>,
    max: i32,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(v) if (1..=max).contains(&v) => ValidationOutcome::pass(),
        _ => ValidationOutcome::fail(rule_id, error_message),
    }
}

/// As [`validate_required_enum`], for an optional field: absence passes,
/// and an explicit `0`/`..._UNSPECIFIED` is invalid exactly like any other
/// undefined discriminant, since a present-but-unspecified value on a
/// field that could simply have been omitted carries no information.
pub fn validate_optional_enum(
    value: Option<i32>,
    max: i32,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        None => ValidationOutcome::pass(),
        Some(v) if (1..=max).contains(&v) => ValidationOutcome::pass(),
        Some(_) => ValidationOutcome::fail(rule_id, error_message),
    }
}

/// As [`validate_required_enum`], for a proto3 field declared *without*
/// the `optional` keyword (so prost represents it as a plain `i32`, not
/// `Option<i32>`): the wire format cannot distinguish "absent" from
/// "explicitly 0/`..._UNSPECIFIED`" for such a field, so 0 must be
/// accepted as legitimate here -- only a genuinely undefined discriminant
/// is invalid.
pub fn validate_implicit_enum(
    value: i32,
    max: i32,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        0 => ValidationOutcome::pass(),
        v if (1..=max).contains(&v) => ValidationOutcome::pass(),
        _ => ValidationOutcome::fail(rule_id, error_message),
    }
}

/// The protocol mixes `float` (`f32`) and `double` (`f64`) fields (compare
/// `Duration.value` to `Location.x`); this lets [`validate_finite`]/
/// [`validate_finite_non_negative`] serve both without duplicating them.
pub trait FloatField: Copy + PartialOrd {
    const FIELD_ZERO: Self;
    fn is_finite_value(self) -> bool;
}

impl FloatField for f32 {
    const FIELD_ZERO: Self = 0.0;
    fn is_finite_value(self) -> bool {
        self.is_finite()
    }
}

impl FloatField for f64 {
    const FIELD_ZERO: Self = 0.0;
    fn is_finite_value(self) -> bool {
        self.is_finite()
    }
}

/// Reject `NaN`/`+-infinity` for a floating-point field, when present.
/// Absence is not this function's concern -- callers check presence
/// separately wherever a field is mandatory. No range beyond finiteness is
/// enforced here: fields whose valid range depends on an external
/// convention (e.g. degrees vs. radians for an angle) are deliberately
/// left alone rather than have this harness invent a policy the schema
/// itself never states.
pub fn validate_finite<T: FloatField>(
    value: Option<T>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(v) if !v.is_finite_value() => ValidationOutcome::fail(rule_id, error_message),
        _ => ValidationOutcome::pass(),
    }
}

/// As [`validate_finite`], and additionally rejects negative values -- for
/// quantities (distance, frequency, amplitude) that are never negative
/// under any unit or coordinate convention, unlike an angle or a signed
/// coordinate.
pub fn validate_finite_non_negative<T: FloatField>(
    value: Option<T>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(v) if !v.is_finite_value() || v < T::FIELD_ZERO => {
            ValidationOutcome::fail(rule_id, error_message)
        }
        _ => ValidationOutcome::pass(),
    }
}

/// `LocationCoordinateSystem` values 3 and 4 are `reserved` in
/// `location.proto` (used up to SAPIENT v7, dropped for non-SI units) --
/// they're still valid `int32`s on the wire, so a plain nonzero check
/// wrongly accepts them. Whitelist the values that are actually defined.
const VALID_LOCATION_COORDINATE_SYSTEMS: [i32; 3] = [1, 2, 5];

/// Same issue as [`VALID_LOCATION_COORDINATE_SYSTEMS`], for
/// `RangeBearingCoordinateSystem` (`range_bearing.proto` reserves 5 and 6).
const VALID_RANGE_BEARING_COORDINATE_SYSTEMS: [i32; 4] = [1, 2, 3, 4];

pub fn validate_location_coordinate_system(
    value: Option<i32>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(v) if VALID_LOCATION_COORDINATE_SYSTEMS.contains(&v) => ValidationOutcome::pass(),
        _ => ValidationOutcome::fail(rule_id, error_message),
    }
}

pub fn validate_range_bearing_coordinate_system(
    value: Option<i32>,
    rule_id: impl Into<String>,
    error_message: &str,
) -> ValidationOutcome {
    match value {
        Some(v) if VALID_RANGE_BEARING_COORDINATE_SYSTEMS.contains(&v) => ValidationOutcome::pass(),
        _ => ValidationOutcome::fail(rule_id, error_message),
    }
}

pub fn validate_associated_detection(
    associated_detection: AssociatedDetection,
    rule_id_prefix: &str,
    node_id_error: &str,
    object_id_error: &str,
) -> ValidationOutcome {
    let node_id_validation = validate_uuid_v4(
        associated_detection.node_id.as_deref(),
        format!("{rule_id_prefix}.node_id.invalid"),
        node_id_error,
    );
    if !node_id_validation.passed {
        return node_id_validation;
    }

    let object_id_validation = validate_ulid(
        associated_detection.object_id.as_deref(),
        format!("{rule_id_prefix}.object_id.invalid"),
        object_id_error,
    );
    if !object_id_validation.passed {
        return object_id_validation;
    }

    // `AssociationRelation` is optional and has no reserved gaps (0-4).
    validate_optional_enum(
        associated_detection.association_type,
        4,
        format!("{rule_id_prefix}.association_type.invalid"),
        "Association type is not a valid option in associated detection.",
    )
}

pub fn validate_associated_file(
    associated_file: AssociatedFile,
    rule_id_prefix: &str,
    type_error: &str,
    url_error: &str,
) -> ValidationOutcome {
    let type_validation = validate_required_string(
        associated_file.r#type.as_deref(),
        format!("{rule_id_prefix}.type.missing"),
        type_error,
    );
    if !type_validation.passed {
        return type_validation;
    }

    let url_validation = validate_required_string(
        associated_file.url.as_deref(),
        format!("{rule_id_prefix}.url.missing"),
        url_error,
    );
    if !url_validation.passed {
        return url_validation;
    }

    ValidationOutcome::pass()
}

pub fn validate_location(location: Location, rule_id_prefix: &str) -> ValidationOutcome {
    if location.x.is_none() {
        return ValidationOutcome::fail(
            format!("{rule_id_prefix}.x.missing"),
            "X-coordinate must be specified in location.",
        );
    }

    if location.y.is_none() {
        return ValidationOutcome::fail(
            format!("{rule_id_prefix}.y.missing"),
            "Y-coordinate must be specified in location.",
        );
    }

    for (value, field) in [
        (location.x, "x"),
        (location.y, "y"),
        (location.z, "z"),
        (location.x_error, "x_error"),
        (location.y_error, "y_error"),
        (location.z_error, "z_error"),
    ] {
        let finite_validation = validate_finite(
            value,
            format!("{rule_id_prefix}.{field}.invalid"),
            "Coordinate must be a finite number in location.",
        );
        if !finite_validation.passed {
            return finite_validation;
        }
    }

    let coordinate_system_validation = validate_location_coordinate_system(
        location.coordinate_system,
        format!("{rule_id_prefix}.coordinate_system.invalid"),
        "Coordinate system must be specified in location.",
    );
    if !coordinate_system_validation.passed {
        return coordinate_system_validation;
    }

    // `LocationDatum` is mandatory and has no reserved gaps (0-2).
    validate_required_enum(
        location.datum,
        2,
        format!("{rule_id_prefix}.datum.missing"),
        "Datum must be specified in location.",
    )
}

pub fn validate_range_bearing(
    range_bearing: RangeBearing,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    for (value, field) in [
        (range_bearing.elevation, "elevation"),
        (range_bearing.azimuth, "azimuth"),
        (range_bearing.elevation_error, "elevation_error"),
        (range_bearing.azimuth_error, "azimuth_error"),
    ] {
        let finite_validation = validate_finite(
            value,
            format!("{rule_id_prefix}.{field}.invalid"),
            "Value must be a finite number in range bearing.",
        );
        if !finite_validation.passed {
            return finite_validation;
        }
    }
    for (value, field) in [
        (range_bearing.range, "range"),
        (range_bearing.range_error, "range_error"),
    ] {
        let range_validation = validate_finite_non_negative(
            value,
            format!("{rule_id_prefix}.{field}.invalid"),
            "Range must be a finite number 0 or greater in range bearing.",
        );
        if !range_validation.passed {
            return range_validation;
        }
    }

    let coordinate_system_validation = validate_range_bearing_coordinate_system(
        range_bearing.coordinate_system,
        format!("{rule_id_prefix}.coordinate_system.invalid"),
        "Coordinate system must be specified in range bearing.",
    );
    if !coordinate_system_validation.passed {
        return coordinate_system_validation;
    }

    // `RangeBearingDatum` is mandatory and has no reserved gaps (0-4).
    validate_required_enum(
        range_bearing.datum,
        4,
        format!("{rule_id_prefix}.datum.missing"),
        "Datum must be specified in range bearing.",
    )
}

pub fn validate_range_bearing_cone(
    range_bearing: RangeBearingCone,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    for (value, field) in [
        (range_bearing.elevation, "elevation"),
        (range_bearing.azimuth, "azimuth"),
        (range_bearing.elevation_error, "elevation_error"),
        (range_bearing.azimuth_error, "azimuth_error"),
        (range_bearing.horizontal_extent, "horizontal_extent"),
        (range_bearing.vertical_extent, "vertical_extent"),
        (
            range_bearing.horizontal_extent_error,
            "horizontal_extent_error",
        ),
        (range_bearing.vertical_extent_error, "vertical_extent_error"),
    ] {
        let finite_validation = validate_finite(
            value,
            format!("{rule_id_prefix}.{field}.invalid"),
            "Value must be a finite number in range bearing.",
        );
        if !finite_validation.passed {
            return finite_validation;
        }
    }
    for (value, field) in [
        (range_bearing.range, "range"),
        (range_bearing.range_error, "range_error"),
    ] {
        let range_validation = validate_finite_non_negative(
            value,
            format!("{rule_id_prefix}.{field}.invalid"),
            "Range must be a finite number 0 or greater in range bearing.",
        );
        if !range_validation.passed {
            return range_validation;
        }
    }

    let coordinate_system_validation = validate_range_bearing_coordinate_system(
        range_bearing.coordinate_system,
        format!("{rule_id_prefix}.coordinate_system.invalid"),
        "Coordinate system must be specified in range bearing.",
    );
    if !coordinate_system_validation.passed {
        return coordinate_system_validation;
    }

    // `RangeBearingDatum` is mandatory and has no reserved gaps (0-4).
    validate_required_enum(
        range_bearing.datum,
        4,
        format!("{rule_id_prefix}.datum.missing"),
        "Datum must be specified in range bearing.",
    )
}

pub fn validate_location_or_range_bearing(
    location_or_range_bearing: LocationOrRangeBearing,
    rule_id_prefix: &str,
    missing_error: &str,
) -> ValidationOutcome {
    match location_or_range_bearing.fov_oneof {
        Some(crate::bsi_flex_335_v2_0::location_or_range_bearing::FovOneof::RangeBearing(
            range_bearing,
        )) => validate_range_bearing_cone(range_bearing, rule_id_prefix),
        Some(crate::bsi_flex_335_v2_0::location_or_range_bearing::FovOneof::LocationList(
            location_list,
        )) => validate_location_list(location_list, rule_id_prefix),
        None => ValidationOutcome::fail(format!("{rule_id_prefix}.missing"), missing_error),
    }
}

pub fn validate_location_list(
    location_list: LocationList,
    rule_id_prefix: &str,
) -> ValidationOutcome {
    if location_list.locations.is_empty() {
        return ValidationOutcome::fail(
            format!("{rule_id_prefix}.locations.empty"),
            "At least one location must be specified in location list.",
        );
    }

    for location in location_list.locations {
        let validation = validate_location(location, &format!("{rule_id_prefix}.locations"));
        if !validation.passed {
            return validation;
        }
    }

    ValidationOutcome::pass()
}

pub fn validate_follow_object(follow_object: FollowObject) -> ValidationOutcome {
    validate_ulid(
        Some(follow_object.follow_object_id.as_str()),
        "task.follow_object.follow_object_id.invalid",
        "A valid ULID must be used for a follow object ID in a follow object message.",
    )
}

#[cfg(test)]
mod common_validation_tests {
    use crate::bsi_flex_335_v2_0::{
        AssociatedDetection, AssociatedFile, FollowObject, Location, LocationList,
        LocationOrRangeBearing, RangeBearing, RangeBearingCone,
        location_or_range_bearing::FovOneof,
    };
    use crate::finding::ValidationOutcome;

    use super::{
        validate_associated_detection, validate_associated_file, validate_finite,
        validate_finite_non_negative, validate_follow_object, validate_implicit_enum,
        validate_location, validate_location_list, validate_location_or_range_bearing,
        validate_nonzero, validate_optional_enum, validate_range_bearing,
        validate_range_bearing_cone, validate_required_enum, validate_required_nonzero,
        validate_required_string, validate_timestamp, validate_ulid, validate_unit_interval,
        validate_uuid_v4,
    };

    #[test]
    fn test_validate_ulid() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_ulid(
                Some("01H1VV3VN40RV97CDFSXJB44K9"),
                "test.ulid",
                "invalid ulid"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.ulid", "invalid ulid"),
            validate_ulid(None, "test.ulid", "invalid ulid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.ulid", "invalid ulid"),
            validate_ulid(
                Some("01h1vv3vn40rv97cdfsxjb44k9"),
                "test.ulid",
                "invalid ulid"
            )
        );
    }

    #[test]
    fn test_validate_uuid_v4() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_uuid_v4(
                Some("550e8400-e29b-41d4-a716-446655440000"),
                "test.uuid",
                "invalid uuid"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.uuid", "invalid uuid"),
            validate_uuid_v4(Some("not-a-uuid"), "test.uuid", "invalid uuid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.uuid", "invalid uuid"),
            validate_uuid_v4(
                Some("550E8400-E29B-41D4-A716-446655440000"),
                "test.uuid",
                "invalid uuid"
            )
        );
    }

    #[test]
    fn test_validate_required_string() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_required_string(Some("value"), "test.string", "required string")
        );
        assert_eq!(
            ValidationOutcome::fail("test.string", "required string"),
            validate_required_string(Some(""), "test.string", "required string")
        );
        assert_eq!(
            ValidationOutcome::fail("test.string", "required string"),
            validate_required_string(None, "test.string", "required string")
        );
    }

    #[test]
    fn test_validate_required_nonzero() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_required_nonzero(Some(1), "test.enum", "required enum")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "required enum"),
            validate_required_nonzero(Some(0), "test.enum", "required enum")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "required enum"),
            validate_required_nonzero(None, "test.enum", "required enum")
        );
    }

    #[test]
    fn test_validate_nonzero() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_nonzero(1, "test.nonzero", "nonzero")
        );
        assert_eq!(
            ValidationOutcome::fail("test.nonzero", "nonzero"),
            validate_nonzero(0, "test.nonzero", "nonzero")
        );
    }

    #[test]
    fn test_validate_timestamp() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: 1,
                    nanos: 0,
                }),
                "test.timestamp.missing",
                "missing timestamp",
                "test.timestamp.malformed",
                "bad timestamp"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.timestamp.missing", "missing timestamp"),
            validate_timestamp(
                None,
                "test.timestamp.missing",
                "missing timestamp",
                "test.timestamp.malformed",
                "bad timestamp"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.timestamp.malformed", "bad timestamp"),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: 1,
                    nanos: 1_000_000_000,
                }),
                "test.timestamp.missing",
                "missing timestamp",
                "test.timestamp.malformed",
                "bad timestamp"
            )
        );
    }

    #[test]
    fn test_validate_associated_detection() {
        let associated_detection = AssociatedDetection {
            timestamp: None,
            node_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            association_type: None,
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_associated_detection(
                associated_detection,
                "test.associated_detection",
                "bad node id",
                "bad object id"
            )
        );
    }

    #[test]
    fn test_validate_unit_interval() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_unit_interval(Some(0.0), "test.scalar", "bad scalar")
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_unit_interval(Some(1.0), "test.scalar", "bad scalar")
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_unit_interval(None, "test.scalar", "bad scalar")
        );
        assert_eq!(
            ValidationOutcome::fail("test.scalar", "bad scalar"),
            validate_unit_interval(Some(-0.1), "test.scalar", "bad scalar")
        );
        assert_eq!(
            ValidationOutcome::fail("test.scalar", "bad scalar"),
            validate_unit_interval(Some(1.1), "test.scalar", "bad scalar")
        );
    }

    #[test]
    fn test_validate_associated_file() {
        let associated_file = AssociatedFile {
            r#type: Some("image/jpeg".to_string()),
            url: Some("https://example.test/file.jpg".to_string()),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_associated_file(
                associated_file,
                "test.associated_file",
                "bad type",
                "bad url"
            )
        );
    }

    #[test]
    fn test_validate_range_bearing_cone() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_range_bearing_cone(
                RangeBearingCone {
                    elevation: Some(1.0),
                    azimuth: Some(2.0),
                    range: Some(3.0),
                    horizontal_extent: None,
                    vertical_extent: None,
                    horizontal_extent_error: None,
                    vertical_extent_error: None,
                    elevation_error: None,
                    azimuth_error: None,
                    range_error: None,
                    coordinate_system: Some(1),
                    datum: Some(1),
                },
                "test.range_bearing_cone"
            )
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_range_bearing_cone(
                RangeBearingCone {
                    elevation: None,
                    azimuth: None,
                    range: None,
                    horizontal_extent: None,
                    vertical_extent: None,
                    horizontal_extent_error: None,
                    vertical_extent_error: None,
                    elevation_error: None,
                    azimuth_error: None,
                    range_error: None,
                    coordinate_system: Some(1),
                    datum: Some(1),
                },
                "test.range_bearing_cone"
            )
        );
        assert_eq!(
            ValidationOutcome::fail(
                "test.range_bearing_cone.coordinate_system.invalid",
                "Coordinate system must be specified in range bearing."
            ),
            validate_range_bearing_cone(
                RangeBearingCone {
                    elevation: Some(1.0),
                    azimuth: Some(2.0),
                    range: Some(3.0),
                    horizontal_extent: None,
                    vertical_extent: None,
                    horizontal_extent_error: None,
                    vertical_extent_error: None,
                    elevation_error: None,
                    azimuth_error: None,
                    range_error: None,
                    coordinate_system: None,
                    datum: Some(1),
                },
                "test.range_bearing_cone"
            )
        );
        assert_eq!(
            ValidationOutcome::fail(
                "test.range_bearing_cone.datum.missing",
                "Datum must be specified in range bearing."
            ),
            validate_range_bearing_cone(
                RangeBearingCone {
                    elevation: Some(1.0),
                    azimuth: Some(2.0),
                    range: Some(3.0),
                    horizontal_extent: None,
                    vertical_extent: None,
                    horizontal_extent_error: None,
                    vertical_extent_error: None,
                    elevation_error: None,
                    azimuth_error: None,
                    range_error: None,
                    coordinate_system: Some(1),
                    datum: None,
                },
                "test.range_bearing_cone"
            )
        );
    }

    #[test]
    fn test_validate_location_or_range_bearing() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location_or_range_bearing(
                LocationOrRangeBearing {
                    fov_oneof: Some(FovOneof::LocationList(LocationList {
                        locations: vec![Location {
                            x: Some(1.0),
                            y: Some(2.0),
                            z: None,
                            x_error: None,
                            y_error: None,
                            z_error: None,
                            coordinate_system: Some(1),
                            datum: Some(1),
                            utm_zone: None,
                        }],
                    })),
                },
                "test.field_of_view",
                "missing"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.field_of_view.missing", "missing"),
            validate_location_or_range_bearing(
                LocationOrRangeBearing { fov_oneof: None },
                "test.field_of_view",
                "missing"
            )
        );
    }

    #[test]
    fn test_validate_location() {
        let location = Location {
            x: Some(1.0),
            y: Some(2.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
            utm_zone: None,
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location(location, "test.location")
        );
    }

    #[test]
    fn test_validate_range_bearing() {
        let range_bearing = RangeBearing {
            elevation: Some(1.0),
            azimuth: None,
            range: None,
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_range_bearing(range_bearing, "test.range_bearing")
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_range_bearing(
                RangeBearing {
                    elevation: None,
                    azimuth: None,
                    range: None,
                    elevation_error: None,
                    azimuth_error: None,
                    range_error: None,
                    coordinate_system: Some(1),
                    datum: Some(1),
                },
                "test.range_bearing"
            )
        );
    }

    #[test]
    fn test_validate_location_list() {
        let location = Location {
            x: Some(1.0),
            y: Some(2.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
            utm_zone: None,
        };
        let location_list = LocationList {
            locations: vec![location],
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_location_list(location_list, "test.location_list")
        );
        assert_eq!(
            ValidationOutcome::fail(
                "test.location_list.locations.coordinate_system.invalid",
                "Coordinate system must be specified in location."
            ),
            validate_location_list(
                LocationList {
                    locations: vec![Location {
                        x: Some(1.0),
                        y: Some(2.0),
                        z: None,
                        x_error: None,
                        y_error: None,
                        z_error: None,
                        coordinate_system: None,
                        datum: Some(1),
                        utm_zone: None,
                    }],
                },
                "test.location_list"
            )
        );
    }

    #[test]
    fn test_validate_follow_object() {
        let follow_object = FollowObject {
            follow_object_id: "01H1VV3VN40RV97CDFSXJB44K9".to_string(),
        };
        assert_eq!(
            ValidationOutcome::pass(),
            validate_follow_object(follow_object)
        );
    }

    #[test]
    fn test_validate_finite() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_finite(Some(1.0_f32), "test.finite", "not finite")
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_finite(None::<f32>, "test.finite", "not finite")
        );
        assert_eq!(
            ValidationOutcome::fail("test.finite", "not finite"),
            validate_finite(Some(f32::NAN), "test.finite", "not finite")
        );
        assert_eq!(
            ValidationOutcome::fail("test.finite", "not finite"),
            validate_finite(Some(f32::INFINITY), "test.finite", "not finite")
        );
        assert_eq!(
            ValidationOutcome::fail("test.finite", "not finite"),
            validate_finite(Some(f64::NEG_INFINITY), "test.finite", "not finite")
        );
    }

    #[test]
    fn test_validate_finite_non_negative() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_finite_non_negative(Some(0.0_f32), "test.range", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.range", "invalid"),
            validate_finite_non_negative(Some(-0.1_f32), "test.range", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.range", "invalid"),
            validate_finite_non_negative(Some(f32::NAN), "test.range", "invalid")
        );
    }

    #[test]
    fn test_validate_required_enum() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_required_enum(Some(1), 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "invalid"),
            validate_required_enum(Some(0), 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "invalid"),
            validate_required_enum(Some(999), 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "invalid"),
            validate_required_enum(None, 4, "test.enum", "invalid")
        );
    }

    #[test]
    fn test_validate_optional_enum() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_optional_enum(None, 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_optional_enum(Some(1), 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "invalid"),
            validate_optional_enum(Some(0), 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "invalid"),
            validate_optional_enum(Some(999), 4, "test.enum", "invalid")
        );
    }

    #[test]
    fn test_validate_implicit_enum() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_implicit_enum(0, 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::pass(),
            validate_implicit_enum(1, 4, "test.enum", "invalid")
        );
        assert_eq!(
            ValidationOutcome::fail("test.enum", "invalid"),
            validate_implicit_enum(999, 4, "test.enum", "invalid")
        );
    }

    #[test]
    fn test_validate_uuid_v4_rejects_non_rfc4122_variant() {
        // Version-4 nibble and canonical formatting, but the variant bits
        // (0x0 in the leading nibble of the clock-seq byte) do not identify
        // an RFC 4122 UUID -- only 0x8-0xb do.
        assert_eq!(
            ValidationOutcome::fail("test.uuid", "invalid uuid"),
            validate_uuid_v4(
                Some("550e8400-e29b-41d4-0716-446655440000"),
                "test.uuid",
                "invalid uuid"
            )
        );
    }

    #[test]
    fn test_validate_timestamp_rejects_out_of_range_seconds() {
        assert_eq!(
            ValidationOutcome::pass(),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: 253_402_300_799,
                    nanos: 0,
                }),
                "test.timestamp.missing",
                "missing timestamp",
                "test.timestamp.malformed",
                "bad timestamp"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.timestamp.malformed", "bad timestamp"),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: i64::MAX,
                    nanos: 0,
                }),
                "test.timestamp.missing",
                "missing timestamp",
                "test.timestamp.malformed",
                "bad timestamp"
            )
        );
        assert_eq!(
            ValidationOutcome::fail("test.timestamp.malformed", "bad timestamp"),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: i64::MIN,
                    nanos: 0,
                }),
                "test.timestamp.missing",
                "missing timestamp",
                "test.timestamp.malformed",
                "bad timestamp"
            )
        );
    }

    #[test]
    fn test_validate_location_rejects_non_finite_coordinates() {
        let location = Location {
            x: Some(f64::NAN),
            y: Some(2.0),
            z: None,
            x_error: None,
            y_error: None,
            z_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
            utm_zone: None,
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.location.x.invalid",
                "Coordinate must be a finite number in location."
            ),
            validate_location(location, "test.location")
        );
    }

    #[test]
    fn test_validate_range_bearing_rejects_negative_range() {
        let range_bearing = RangeBearing {
            elevation: Some(1.0),
            azimuth: Some(2.0),
            range: Some(-1.0),
            elevation_error: None,
            azimuth_error: None,
            range_error: None,
            coordinate_system: Some(1),
            datum: Some(1),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.range_bearing.range.invalid",
                "Range must be a finite number 0 or greater in range bearing."
            ),
            validate_range_bearing(range_bearing, "test.range_bearing")
        );
    }

    #[test]
    fn test_validate_associated_detection_rejects_invalid_association_type() {
        let associated_detection = AssociatedDetection {
            timestamp: None,
            node_id: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            object_id: Some("01H1VV3VN40RV97CDFSXJB44K9".to_string()),
            association_type: Some(999),
        };
        assert_eq!(
            ValidationOutcome::fail(
                "test.associated_detection.association_type.invalid",
                "Association type is not a valid option in associated detection."
            ),
            validate_associated_detection(
                associated_detection,
                "test.associated_detection",
                "bad node id",
                "bad object id"
            )
        );
    }
}
