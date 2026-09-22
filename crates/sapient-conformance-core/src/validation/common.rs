use ulid::Ulid;
use uuid::Uuid;

use crate::bsi_flex_335_v2_0::{
    AssociatedDetection, AssociatedFile, FollowObject, Location, LocationList,
    LocationOrRangeBearing, RangeBearing, RangeBearingCone,
};
use prost_types::Timestamp;

pub fn validate_ulid(id: Option<&str>, error_message: &str) -> (bool, String) {
    let value = match id {
        Some(value) => value,
        None => return (false, error_message.to_string()),
    };

    let parsed = match Ulid::from_string(value) {
        Ok(parsed) => parsed,
        Err(_) => return (false, error_message.to_string()),
    };

    if parsed.to_string() != value {
        return (false, error_message.to_string());
    }

    (true, String::new())
}

pub fn validate_uuid_v4(id: Option<&str>, error_message: &str) -> (bool, String) {
    let value = match id {
        Some(value) => value,
        None => return (false, error_message.to_string()),
    };

    let parsed = Uuid::parse_str(value);

    match parsed {
        Ok(uuid) if uuid.get_version_num() == 4 && uuid.hyphenated().to_string() == value => {
            (true, String::new())
        }
        _ => (false, error_message.to_string()),
    }
}

pub fn validate_timestamp(
    timestamp: Option<Timestamp>,
    missing_error_message: &str,
    malformed_error_message: &str,
) -> (bool, String) {
    let timestamp = match timestamp {
        Some(timestamp) => timestamp,
        None => return (false, missing_error_message.to_string()),
    };

    if !(0..1_000_000_000).contains(&timestamp.nanos) {
        return (false, malformed_error_message.to_string());
    }

    (true, String::new())
}

pub fn validate_unit_interval(value: Option<f32>, error_message: &str) -> (bool, String) {
    match value {
        Some(value) if (0.0..=1.0).contains(&value) => (true, String::new()),
        Some(_) => (false, error_message.to_string()),
        None => (true, String::new()),
    }
}

pub fn validate_required_string(value: Option<&str>, error_message: &str) -> (bool, String) {
    match value {
        Some("") | None => (false, error_message.to_string()),
        Some(_) => (true, String::new()),
    }
}

pub fn validate_required_nonzero(value: Option<i32>, error_message: &str) -> (bool, String) {
    match value {
        None | Some(0) => (false, error_message.to_string()),
        Some(_) => (true, String::new()),
    }
}

pub fn validate_nonzero(value: i32, error_message: &str) -> (bool, String) {
    match value {
        0 => (false, error_message.to_string()),
        _ => (true, String::new()),
    }
}

pub fn validate_associated_detection(
    associated_detection: AssociatedDetection,
    node_id_error: &str,
    object_id_error: &str,
) -> (bool, String) {
    let node_id_validation =
        validate_uuid_v4(associated_detection.node_id.as_deref(), node_id_error);
    if !node_id_validation.0 {
        return node_id_validation;
    }

    validate_ulid(associated_detection.object_id.as_deref(), object_id_error)
}

pub fn validate_associated_file(
    associated_file: AssociatedFile,
    type_error: &str,
    url_error: &str,
) -> (bool, String) {
    let type_validation = validate_required_string(associated_file.r#type.as_deref(), type_error);
    if !type_validation.0 {
        return type_validation;
    }

    let url_validation = validate_required_string(associated_file.url.as_deref(), url_error);
    if !url_validation.0 {
        return url_validation;
    }

    (true, String::new())
}

pub fn validate_location(location: Location) -> (bool, String) {
    if location.x.is_none() {
        return (
            false,
            "X-coordinate must be specified in location.".to_string(),
        );
    }

    if location.y.is_none() {
        return (
            false,
            "Y-coordinate must be specified in location.".to_string(),
        );
    }

    let coordinate_system_validation = validate_required_nonzero(
        location.coordinate_system,
        "Coordinate system must be specified in location.",
    );
    if !coordinate_system_validation.0 {
        return coordinate_system_validation;
    }

    let datum_validation =
        validate_required_nonzero(location.datum, "Datum must be specified in location.");
    if !datum_validation.0 {
        return datum_validation;
    }

    (true, String::new())
}

pub fn validate_range_bearing(range_bearing: RangeBearing) -> (bool, String) {
    let coordinate_system_validation = validate_required_nonzero(
        range_bearing.coordinate_system,
        "Coordinate system must be specified in range bearing.",
    );
    if !coordinate_system_validation.0 {
        return coordinate_system_validation;
    }

    let datum_validation = validate_required_nonzero(
        range_bearing.datum,
        "Datum must be specified in range bearing.",
    );
    if !datum_validation.0 {
        return datum_validation;
    }

    (true, String::new())
}

pub fn validate_range_bearing_cone(range_bearing: RangeBearingCone) -> (bool, String) {
    let coordinate_system_validation = validate_required_nonzero(
        range_bearing.coordinate_system,
        "Coordinate system must be specified in range bearing.",
    );
    if !coordinate_system_validation.0 {
        return coordinate_system_validation;
    }

    let datum_validation = validate_required_nonzero(
        range_bearing.datum,
        "Datum must be specified in range bearing.",
    );
    if !datum_validation.0 {
        return datum_validation;
    }

    (true, String::new())
}

pub fn validate_location_or_range_bearing(
    location_or_range_bearing: LocationOrRangeBearing,
    missing_error: &str,
) -> (bool, String) {
    match location_or_range_bearing.fov_oneof {
        Some(crate::bsi_flex_335_v2_0::location_or_range_bearing::FovOneof::RangeBearing(
            range_bearing,
        )) => validate_range_bearing_cone(range_bearing),
        Some(crate::bsi_flex_335_v2_0::location_or_range_bearing::FovOneof::LocationList(
            location_list,
        )) => validate_location_list(location_list),
        None => (false, missing_error.to_string()),
    }
}

pub fn validate_location_list(location_list: LocationList) -> (bool, String) {
    if location_list.locations.is_empty() {
        return (
            false,
            "At least one location must be specified in location list.".to_string(),
        );
    }

    for location in location_list.locations {
        let validation = validate_location(location);
        if !validation.0 {
            return validation;
        }
    }

    (true, String::new())
}

pub fn validate_follow_object(follow_object: FollowObject) -> (bool, String) {
    validate_ulid(
        Some(follow_object.follow_object_id.as_str()),
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

    use super::{
        validate_associated_detection, validate_associated_file, validate_follow_object,
        validate_location, validate_location_list, validate_location_or_range_bearing,
        validate_nonzero, validate_range_bearing, validate_range_bearing_cone,
        validate_required_nonzero, validate_required_string, validate_timestamp, validate_ulid,
        validate_unit_interval, validate_uuid_v4,
    };

    #[test]
    fn test_validate_ulid() {
        assert_eq!(
            (true, String::new()),
            validate_ulid(Some("01H1VV3VN40RV97CDFSXJB44K9"), "invalid ulid")
        );
        assert_eq!(
            (false, "invalid ulid".to_string()),
            validate_ulid(None, "invalid ulid")
        );
        assert_eq!(
            (false, "invalid ulid".to_string()),
            validate_ulid(Some("01h1vv3vn40rv97cdfsxjb44k9"), "invalid ulid")
        );
    }

    #[test]
    fn test_validate_uuid_v4() {
        assert_eq!(
            (true, String::new()),
            validate_uuid_v4(Some("550e8400-e29b-41d4-a716-446655440000"), "invalid uuid")
        );
        assert_eq!(
            (false, "invalid uuid".to_string()),
            validate_uuid_v4(Some("not-a-uuid"), "invalid uuid")
        );
        assert_eq!(
            (false, "invalid uuid".to_string()),
            validate_uuid_v4(Some("550E8400-E29B-41D4-A716-446655440000"), "invalid uuid")
        );
    }

    #[test]
    fn test_validate_required_string() {
        assert_eq!(
            (true, String::new()),
            validate_required_string(Some("value"), "required string")
        );
        assert_eq!(
            (false, "required string".to_string()),
            validate_required_string(Some(""), "required string")
        );
        assert_eq!(
            (false, "required string".to_string()),
            validate_required_string(None, "required string")
        );
    }

    #[test]
    fn test_validate_required_nonzero() {
        assert_eq!(
            (true, String::new()),
            validate_required_nonzero(Some(1), "required enum")
        );
        assert_eq!(
            (false, "required enum".to_string()),
            validate_required_nonzero(Some(0), "required enum")
        );
        assert_eq!(
            (false, "required enum".to_string()),
            validate_required_nonzero(None, "required enum")
        );
    }

    #[test]
    fn test_validate_nonzero() {
        assert_eq!((true, String::new()), validate_nonzero(1, "nonzero"));
        assert_eq!(
            (false, "nonzero".to_string()),
            validate_nonzero(0, "nonzero")
        );
    }

    #[test]
    fn test_validate_timestamp() {
        assert_eq!(
            (true, String::new()),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: 1,
                    nanos: 0,
                }),
                "missing timestamp",
                "bad timestamp"
            )
        );
        assert_eq!(
            (false, "missing timestamp".to_string()),
            validate_timestamp(None, "missing timestamp", "bad timestamp")
        );
        assert_eq!(
            (false, "bad timestamp".to_string()),
            validate_timestamp(
                Some(prost_types::Timestamp {
                    seconds: 1,
                    nanos: 1_000_000_000,
                }),
                "missing timestamp",
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
            (true, String::new()),
            validate_associated_detection(associated_detection, "bad node id", "bad object id")
        );
    }

    #[test]
    fn test_validate_unit_interval() {
        assert_eq!(
            (true, String::new()),
            validate_unit_interval(Some(0.0), "bad scalar")
        );
        assert_eq!(
            (true, String::new()),
            validate_unit_interval(Some(1.0), "bad scalar")
        );
        assert_eq!(
            (true, String::new()),
            validate_unit_interval(None, "bad scalar")
        );
        assert_eq!(
            (false, "bad scalar".to_string()),
            validate_unit_interval(Some(-0.1), "bad scalar")
        );
        assert_eq!(
            (false, "bad scalar".to_string()),
            validate_unit_interval(Some(1.1), "bad scalar")
        );
    }

    #[test]
    fn test_validate_associated_file() {
        let associated_file = AssociatedFile {
            r#type: Some("image/jpeg".to_string()),
            url: Some("https://example.test/file.jpg".to_string()),
        };
        assert_eq!(
            (true, String::new()),
            validate_associated_file(associated_file, "bad type", "bad url")
        );
    }

    #[test]
    fn test_validate_range_bearing_cone() {
        assert_eq!(
            (true, String::new()),
            validate_range_bearing_cone(RangeBearingCone {
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
            })
        );
        assert_eq!(
            (true, String::new()),
            validate_range_bearing_cone(RangeBearingCone {
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
            })
        );
        assert_eq!(
            (
                false,
                "Coordinate system must be specified in range bearing.".to_string()
            ),
            validate_range_bearing_cone(RangeBearingCone {
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
            })
        );
        assert_eq!(
            (
                false,
                "Datum must be specified in range bearing.".to_string()
            ),
            validate_range_bearing_cone(RangeBearingCone {
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
            })
        );
    }

    #[test]
    fn test_validate_location_or_range_bearing() {
        assert_eq!(
            (true, String::new()),
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
                "missing"
            )
        );
        assert_eq!(
            (false, "missing".to_string()),
            validate_location_or_range_bearing(
                LocationOrRangeBearing { fov_oneof: None },
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
        assert_eq!((true, String::new()), validate_location(location));
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
        assert_eq!((true, String::new()), validate_range_bearing(range_bearing));
        assert_eq!(
            (true, String::new()),
            validate_range_bearing(RangeBearing {
                elevation: None,
                azimuth: None,
                range: None,
                elevation_error: None,
                azimuth_error: None,
                range_error: None,
                coordinate_system: Some(1),
                datum: Some(1),
            })
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
        assert_eq!((true, String::new()), validate_location_list(location_list));
        assert_eq!(
            (
                false,
                "Coordinate system must be specified in location.".to_string()
            ),
            validate_location_list(LocationList {
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
            })
        );
    }

    #[test]
    fn test_validate_follow_object() {
        let follow_object = FollowObject {
            follow_object_id: "01H1VV3VN40RV97CDFSXJB44K9".to_string(),
        };
        assert_eq!((true, String::new()), validate_follow_object(follow_object));
    }
}
