use omen_rs::commands::registry;

#[test]
fn registry_has_expected_commands() {
    let names: Vec<&str> = registry().iter().map(|c| c.name()).collect();
    assert!(names.contains(&"info"));
    assert!(names.contains(&"fan"));
    assert!(names.contains(&"gpu"));
}
