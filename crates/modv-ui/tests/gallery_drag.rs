//! Pure Gallery → Group drag resolution: the payload names the module, the
//! hovered Group names the destination, and a release over empty space
//! cancels without queueing anything.

use modv_ui::gallery_drag::{GalleryDrag, gallery_drop_action};

#[test]
fn a_release_over_a_group_queues_the_double_click_action() {
    let payload = GalleryDrag::new("Bands");
    let action = gallery_drop_action(Some(&payload), Some("group-a")).expect("an action");

    assert_eq!(action.action, "ui/addModuleToGroup");
    assert_eq!(
        action.payload,
        serde_json::json!({ "moduleName": "Bands", "groupId": "group-a" })
    );
}

#[test]
fn the_destination_is_the_hovered_group_not_a_default() {
    let payload = GalleryDrag::new("Ball");
    let action = gallery_drop_action(Some(&payload), Some("group-b")).expect("an action");
    assert_eq!(action.payload["groupId"], "group-b");
}

#[test]
fn a_release_over_empty_space_cancels() {
    let payload = GalleryDrag::new("Bands");
    assert!(gallery_drop_action(Some(&payload), None).is_none());
}

#[test]
fn a_release_without_a_payload_cancels() {
    assert!(gallery_drop_action(None, Some("group-a")).is_none());
}

#[test]
fn the_payload_keeps_the_full_module_name() {
    let payload = GalleryDrag::new("It Is Coming For You");
    assert_eq!(payload.module_name, "It Is Coming For You");
    assert_eq!(
        payload.drop_action("group-a").payload["moduleName"],
        "It Is Coming For You"
    );
}
