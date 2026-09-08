//! Tests for the docking bay path links every loader is expected to hand over already checked.

use pof::*;

/// A model with `count` paths and nothing else, so a bay's link has something to be in range of.
fn model_with_paths(count: u32) -> Model {
    let mut model = Model::default();
    model.paths = (0..count).map(|i| Path { name: format!("$path{:02}", i + 1), ..Default::default() }).collect();
    model
}

#[test]
fn a_dock_link_naming_no_path_is_cleared() {
    let mut model = model_with_paths(2);
    model.docking_bays.push(Dock { path: Some(PathId(0)), ..Default::default() });
    model.docking_bays.push(Dock { path: Some(PathId(1)), ..Default::default() });
    model.docking_bays.push(Dock { path: Some(PathId(2)), ..Default::default() });
    model.docking_bays.push(Dock { path: Some(PathId(7)), ..Default::default() });
    model.docking_bays.push(Dock { path: None, ..Default::default() });

    model.sanitize_dock_paths();

    let links: Vec<_> = model.docking_bays.iter().map(|bay| bay.path).collect();
    assert_eq!(links, vec![Some(PathId(0)), Some(PathId(1)), None, None, None]);
}

#[test]
fn a_link_to_the_last_path_survives() {
    // the boundary the check turns on - one past the last valid index is the first invalid one
    let mut model = model_with_paths(1);
    model.docking_bays.push(Dock { path: Some(PathId(0)), ..Default::default() });
    model.sanitize_dock_paths();
    assert_eq!(model.docking_bays[0].path, Some(PathId(0)));
}

#[test]
fn every_link_goes_when_there_are_no_paths_at_all() {
    // the state an import produces when it read a bay's path number but no #paths node
    let mut model = model_with_paths(0);
    model.docking_bays.push(Dock { path: Some(PathId(0)), ..Default::default() });
    model.sanitize_dock_paths();
    assert_eq!(model.docking_bays[0].path, None);
}

#[test]
fn sanitizing_twice_over_changes_nothing_the_second_time() {
    let mut model = model_with_paths(1);
    model.docking_bays.push(Dock { path: Some(PathId(0)), ..Default::default() });
    model.docking_bays.push(Dock { path: Some(PathId(4)), ..Default::default() });

    model.sanitize_dock_paths();
    let after_once: Vec<_> = model.docking_bays.iter().map(|bay| bay.path).collect();
    model.sanitize_dock_paths();
    let after_twice: Vec<_> = model.docking_bays.iter().map(|bay| bay.path).collect();
    assert_eq!(after_once, after_twice);
}
