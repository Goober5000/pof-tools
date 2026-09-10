use egui::{collapsing_header::CollapsingState, Button, Color32, Id, Response, RichText, TextEdit, TextStyle, Ui, WidgetText};
use pof::{properties_delete_field, PathId, Submodel, SubmodelId, TextureId};

use crate::{
    start_loading_import_model,
    ui::{
        DockingTreeValue, EyeTreeValue, GlowTreeValue, InsigniaTreeValue, PathTreeValue, PofToolsGui, SpecialPointTreeValue, SubmodelTreeValue,
        ThrusterTreeValue, TreeValue, TurretTreeValue, UiState, WeaponTreeValue, ERROR_RED, WARNING_YELLOW,
    },
    LoadingThread, Model,
};

use std::collections::{hash_map::Entry, BTreeSet, HashMap};

#[derive(PartialEq)]
pub enum ImportType {
    Add,
    //ClearAndAdd,
    MatchAndReplace,
}

enum SelectionType {
    Total,
    Partial,
    None,
}

#[derive(Default)]
struct ImportOptions {
    auto_select_smodel_children: bool,
    auto_select_paths: bool,
    auto_select_turrets: bool,
}

/// The state associated to the GUI import window
pub struct ImportWindow {
    /// whether its open
    pub open: bool,
    /// what type of import has been selected (add or match-and-replace)
    pub import_type: ImportType,
    /// the model to be imported (if one has been selected)
    pub model: Option<Box<pof::Model>>,
    /// the thread handling loading of the modle to be imported
    pub import_model_loading_thread: LoadingThread,
    /// the set of tree values corresponding to the individual data structures to import
    pub import_selection: BTreeSet<TreeValue>,
    /// various options concerning selection of items in the GUI
    import_options: ImportOptions,
}
impl Default for ImportWindow {
    fn default() -> Self {
        Self {
            open: Default::default(),
            import_type: ImportType::Add,
            model: Default::default(),
            import_model_loading_thread: Default::default(),
            import_selection: BTreeSet::new(),
            import_options: ImportOptions {
                auto_select_smodel_children: true,
                auto_select_paths: true,
                auto_select_turrets: true,
            },
        }
    }
}

impl UiState {
    pub fn show_import_window(&mut self, model: &Model, ctx: &egui::Context) -> bool {
        let window = egui::Window::new("Import")
            .collapsible(false)
            .resizable(true)
            .open(&mut self.import_window.open)
            .vscroll(true)
            .default_pos([100.0, 100.0]);

        let mut ret = false;

        window.show(ctx, |ui| {
            let selection = &self.import_window.import_selection;
            let num_submodels_selected = selection.iter().filter(|select| matches!(select, TreeValue::Submodels(_))).count();
            let num_pri_banks_selected = selection
                .iter()
                .filter(|select| matches!(select, TreeValue::Weapons(WeaponTreeValue::PriBank(_))))
                .count();
            let num_sec_banks_selected = selection
                .iter()
                .filter(|select| matches!(select, TreeValue::Weapons(WeaponTreeValue::SecBank(_))))
                .count();
            let num_docks_selected = selection.iter().filter(|select| matches!(select, TreeValue::DockingBays(_))).count();
            let num_thruster_banks_selected = selection.iter().filter(|select| matches!(select, TreeValue::Thrusters(_))).count();
            let num_glow_banks_selected = selection.iter().filter(|select| matches!(select, TreeValue::Glows(_))).count();
            let num_spc_points_selected = selection.iter().filter(|select| matches!(select, TreeValue::SpecialPoints(_))).count();
            let num_turrets_selected = selection.iter().filter(|select| matches!(select, TreeValue::Turrets(_))).count();
            let num_paths_selected = selection.iter().filter(|select| matches!(select, TreeValue::Paths(_))).count();
            let num_eyes_selected = selection.iter().filter(|select| matches!(select, TreeValue::EyePoints(_))).count();
            let num_insignias_selected = selection.iter().filter(|select| matches!(select, TreeValue::Insignia(_))).count();

            egui::SidePanel::left("left_panel")
                .default_width(250.0)
                .width_range(80.0..=250.0)
                .show_inside(ui, |ui| {
                    ui.label(
                        "Here you can import pof data from another model, either by adding it or matching it against \
                            existing data and replacing it.",
                    );

                    ui.checkbox(
                        &mut self.import_window.import_options.auto_select_smodel_children,
                        "Auto-select submodel children on submodel selection.",
                    );
                    ui.checkbox(&mut self.import_window.import_options.auto_select_paths, "Auto-select associated paths, if available.");
                    ui.checkbox(&mut self.import_window.import_options.auto_select_turrets, "Auto-select associated turret data, if available.");

                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.import_window.import_type,
                            ImportType::Add,
                            RichText::new("Add").text_style(TextStyle::Heading),
                        );
                        ui.selectable_value(
                            &mut self.import_window.import_type,
                            ImportType::MatchAndReplace,
                            RichText::new("Match And Replace").text_style(TextStyle::Heading),
                        );
                    });

                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        let path_string = self
                            .import_window
                            .model
                            .as_ref()
                            .map_or(String::new(), |model| model.path_to_file.display().to_string());
                        let path_string = match path_string.char_indices().nth_back(36) {
                            Some((n, _)) => format!("...{}", &path_string[(n + 3)..]),
                            None => path_string,
                        };
                        let mut pos = ui.next_widget_position();
                        pos.y -= ui.style().spacing.interact_size.y * 0.5;
                        let rect = egui::Rect::from_min_size(pos, [ui.available_width(), ui.available_height()].into());
                        ui.painter().rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                        let response = ui.add(TextEdit::singleline(&mut &*path_string).hint_text("Model file...").desired_width(220.0));

                        let response = response.interact(egui::Sense::click());
                        let mut clicked_browse = response.clicked();

                        if self.import_window.import_model_loading_thread.is_some() {
                            ui.add(egui::widgets::Spinner::new());
                        } else {
                            clicked_browse |= ui.button("...").clicked();
                        }

                        if clicked_browse {
                            start_loading_import_model(&mut self.import_window.import_model_loading_thread);
                        }
                    });

                    ui.add_space(10.0);

                    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                        let selection = &self.import_window.import_selection;
                        let import_model = &self.import_window.model;

                        if selection.contains(&TreeValue::Header) {
                            ui.label("Header Geometry Data selected.");

                            if ImportType::Add == self.import_window.import_type {
                                ui.indent("header warnings", |ui| {
                                    ui.label(
                                        RichText::new(format!("Header geometry data cannot be added, it will be match and replaced instead."))
                                            .color(WARNING_YELLOW),
                                    );
                                });
                            }
                        }

                        if num_submodels_selected > 0 {
                            ui.label(format!("{} Submodel{} selected.", num_submodels_selected, if num_submodels_selected == 1 { "" } else { "s" }));

                            let mut warnings = vec![];

                            if ImportType::MatchAndReplace == self.import_window.import_type {
                                for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::Submodels(_))) {
                                    if let TreeValue::Submodels(SubmodelTreeValue::Submodel(id)) = tree_val {
                                        if !model
                                            .submodels
                                            .iter()
                                            .any(|smodel| smodel.name == import_model.as_ref().unwrap().submodels[*id].name)
                                        {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "There is no match for submodel '{}' in the recieving model. It will be added instead.",
                                                    import_model.as_ref().unwrap().submodels[*id].name
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("submodel warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_pri_banks_selected > 0 {
                            ui.label(format!(
                                "{} Primary Bank{} selected.",
                                num_pri_banks_selected,
                                if num_pri_banks_selected == 1 { "" } else { "s" }
                            ));
                        }

                        if num_sec_banks_selected > 0 {
                            ui.label(format!(
                                "{} Secondary Bank{} selected.",
                                num_sec_banks_selected,
                                if num_sec_banks_selected == 1 { "" } else { "s" }
                            ));
                        }

                        if num_docks_selected > 0 {
                            ui.label(format!("{} Docking bay{} selected.", num_docks_selected, if num_docks_selected == 1 { "" } else { "s" }));

                            let mut warnings = vec![];

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::DockingBays(_))) {
                                if let TreeValue::DockingBays(DockingTreeValue::Bay(idx)) = tree_val {
                                    let dock = &import_model.as_ref().unwrap().docking_bays[*idx];

                                    if let Some(id) = dock
                                        .get_parent_smodel()
                                        .and_then(|submodel_str| import_model.as_ref().unwrap().get_model_id_by_name(submodel_str))
                                    {
                                        if !self
                                            .import_window
                                            .import_selection
                                            .contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(id)))
                                        {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "Docking bay '{}' has a parent submodel which is not being imported. \
                                                         The parent submodel field will be removed.",
                                                    import_model.as_ref().unwrap().docking_bays[*idx]
                                                        .get_name()
                                                        .unwrap_or(&format!("Docking bay {}", idx)),
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    }

                                    if self.import_window.import_type == ImportType::MatchAndReplace {
                                        if let Some(name) = dock.get_name() {
                                            if !model.docking_bays.iter().any(|other_dock| other_dock.get_name() == Some(name)) {
                                                warnings.push(
                                                    RichText::new(format!(
                                                        "There is no match for docking bay '{}' in the recieving model. \
                                                             It will be added instead.",
                                                        name
                                                    ))
                                                    .color(WARNING_YELLOW),
                                                );
                                            }
                                        } else {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "Docking bay {} needs a name to be properly matched. \
                                                         It will be added instead.",
                                                    idx
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("dock warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_thruster_banks_selected > 0 {
                            ui.label(format!(
                                "{} Thruster bank{} selected.",
                                num_thruster_banks_selected,
                                if num_thruster_banks_selected == 1 { "" } else { "s" }
                            ));

                            let mut warnings = vec![];

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::Thrusters(_))) {
                                if let TreeValue::Thrusters(ThrusterTreeValue::Bank(idx)) = tree_val {
                                    let import_model = &import_model.as_ref().unwrap();
                                    if let Some(subsys_name) = import_model.thruster_banks[*idx].get_engine_subsys() {
                                        let mut found_a_match = import_model.get_model_id_by_name(subsys_name).is_some_and(|id| {
                                            !self
                                                .import_window
                                                .import_selection
                                                .contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(id)))
                                        });

                                        found_a_match |= import_model
                                            .special_points
                                            .iter()
                                            .filter(|spc_point| spc_point.is_subsystem())
                                            .any(|spc_point| spc_point.name.strip_prefix('$').unwrap_or(&spc_point.name) == subsys_name);

                                        if !found_a_match {
                                            // engine subsys was not imported, lose it
                                            warnings.push(
                                                RichText::new(format!(
                                                    "There is no matching engine subsystem for thruster bank {} in the recieving model. \
                                                        It will be added instead.",
                                                    idx + 1
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    } else {
                                        warnings.push(
                                            RichText::new(format!(
                                                "Thruster bank {} needs an associated engine subsystem to be properly matched. \
                                                    It will be added instead.",
                                                idx + 1
                                            ))
                                            .color(WARNING_YELLOW),
                                        );
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("thruster banks warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_glow_banks_selected > 0 {
                            ui.label(format!(
                                "{} Glow bank{} selected.",
                                num_glow_banks_selected,
                                if num_glow_banks_selected == 1 { "" } else { "s" }
                            ));

                            let mut warnings = vec![];

                            if self.import_window.import_type == ImportType::MatchAndReplace {
                                warnings
                                    .push(RichText::new("Glow banks cannt be match and replaced. Any will be added instead.").color(WARNING_YELLOW));
                            }

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::Glows(_))) {
                                if let TreeValue::Glows(GlowTreeValue::Bank(idx)) = tree_val {
                                    let glow_bank = &import_model.as_ref().unwrap().glow_banks[*idx];
                                    if !self
                                        .import_window
                                        .import_selection
                                        .contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(glow_bank.model_parent)))
                                    {
                                        let was_already_on_detail0 =
                                            import_model.as_ref().unwrap().header.detail_levels.first() == Some(&glow_bank.model_parent);
                                        if !was_already_on_detail0 {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "Glow bank {}'s submodel parent is not being imported; \
                                                         it will be reset to the recieving model's detail0.",
                                                    idx + 1
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    };
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("glow banks warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_spc_points_selected > 0 {
                            ui.label(format!(
                                "{} Special point{} selected.",
                                num_spc_points_selected,
                                if num_spc_points_selected == 1 { "" } else { "s" }
                            ));

                            let mut warnings = vec![];

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::SpecialPoints(_))) {
                                if let TreeValue::SpecialPoints(SpecialPointTreeValue::Point(idx)) = tree_val {
                                    let spc_point = &import_model.as_ref().unwrap().special_points[*idx];

                                    if self.import_window.import_type == ImportType::MatchAndReplace {
                                        if !model.special_points.iter().any(|other_point| other_point.name == spc_point.name) {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "There is no match for special point '{}' in the recieving model. It will be added instead.",
                                                    spc_point.name
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("special point warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_turrets_selected > 0 {
                            ui.label(format!("{} Turret{} selected.", num_turrets_selected, if num_turrets_selected == 1 { "" } else { "s" }));

                            let mut warnings = vec![];

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::Turrets(_))) {
                                if let TreeValue::Turrets(TurretTreeValue::Turret(idx)) = tree_val {
                                    let import_model = import_model.as_ref().unwrap();
                                    let turret = &import_model.turrets[*idx];

                                    match self.import_window.import_type {
                                        ImportType::Add => {
                                            if !selection.contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(turret.base_model)))
                                                || !selection.contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(turret.gun_model)))
                                            {
                                                // parent submodels were not imported, see if we can find parents...
                                                let mut found_match = false;
                                                for smodel in &model.submodels {
                                                    let singlepart_valid = smodel.name == import_model.submodels[turret.gun_model].name
                                                        && turret.gun_model == turret.base_model;
                                                    let multipart_valid = smodel.name == import_model.submodels[turret.gun_model].name
                                                        && smodel.parent().map(|id| &model.submodels[id].name)
                                                            == Some(&import_model.submodels[turret.base_model].name);

                                                    found_match = singlepart_valid || multipart_valid;
                                                    if found_match {
                                                        break;
                                                    }
                                                }

                                                if !found_match {
                                                    warnings.push(
                                                        RichText::new(format!(
                                                            "There is no match for turret '{}' in the recieving model. It will not be added.",
                                                            import_model.submodels[turret.base_model].name
                                                        ))
                                                        .color(ERROR_RED),
                                                    );
                                                }
                                            }
                                        }
                                        ImportType::MatchAndReplace => {
                                            if !model.turrets.iter().any(|other_turret| {
                                                model.submodels[other_turret.base_model].name == import_model.submodels[turret.base_model].name
                                            }) {
                                                if !selection.contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(turret.base_model))) {
                                                    warnings.push(
                                                        RichText::new(format!(
                                                            "There is no match for turret '{}' in the recieving model. It will not be added.",
                                                            import_model.submodels[turret.base_model].name
                                                        ))
                                                        .color(ERROR_RED),
                                                    );
                                                } else {
                                                    warnings.push(
                                                        RichText::new(format!(
                                                            "There is no match for turret '{}' in the recieving model. It will be added instead.",
                                                            import_model.submodels[turret.base_model].name
                                                        ))
                                                        .color(WARNING_YELLOW),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("turret warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_paths_selected > 0 {
                            ui.label(format!("{} Path{} selected.", num_paths_selected, if num_paths_selected == 1 { "" } else { "s" }));

                            let mut warnings = vec![];

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::Paths(_))) {
                                if let TreeValue::Paths(PathTreeValue::Path(idx)) = tree_val {
                                    let path = &import_model.as_ref().unwrap().paths[*idx];

                                    if self.import_window.import_type == ImportType::MatchAndReplace {
                                        if !model.paths.iter().any(|other_path| other_path.name == path.name) {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "There is no match for path '{}' in the recieving model. It will be added instead.",
                                                    path.name
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("turret warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_eyes_selected > 0 {
                            ui.label(format!("{} Eye point{} selected.", num_eyes_selected, if num_eyes_selected == 1 { "" } else { "s" }));

                            let mut warnings = vec![];

                            for tree_val in selection.iter().filter(|select| matches!(select, TreeValue::EyePoints(_))) {
                                if let TreeValue::EyePoints(EyeTreeValue::EyePoint(idx)) = tree_val {
                                    let point = &import_model.as_ref().unwrap().eye_points[*idx];

                                    if self.import_window.import_type == ImportType::MatchAndReplace {
                                        let attached_smodel = point.attached_submodel.map(|id| &import_model.as_ref().unwrap().submodels[id].name);

                                        if !model.eye_points.iter().any(|other_point| {
                                            other_point.attached_submodel.map(|id| &model.pof_model.submodels[id].name) == attached_smodel
                                        }) {
                                            warnings.push(
                                                RichText::new(format!(
                                                    "There is no match for an eye point in the recieving model. It will be added instead."
                                                ))
                                                .color(WARNING_YELLOW),
                                            );
                                        }
                                    }
                                }
                            }

                            if !warnings.is_empty() {
                                ui.indent("eye warnings", |ui| {
                                    for warning in warnings {
                                        ui.label(warning);
                                    }
                                });
                            }
                        }

                        if num_insignias_selected > 0 {
                            ui.label(format!("{} Insignia selected.", num_insignias_selected));

                            if self.import_window.import_type == ImportType::MatchAndReplace
                                && selection.iter().any(|select| matches!(select, TreeValue::Insignia(_)))
                            {
                                ui.indent("insignia warnings", |ui| {
                                    ui.label(
                                        RichText::new(format!("Insignia cannot be match and replaced, any selected will be added instead."))
                                            .color(WARNING_YELLOW),
                                    );
                                });
                            }
                        }

                        if self.import_window.import_selection.contains(&TreeValue::Shield) {
                            ui.label(format!("Shield selected."));

                            if self.import_window.import_type == ImportType::Add && selection.iter().any(|select| matches!(select, TreeValue::Shield))
                            {
                                ui.indent("shield warnings", |ui| {
                                    ui.label(
                                        RichText::new(format!("The shield cannot be added, it will be match and replaced instead."))
                                            .color(WARNING_YELLOW),
                                    );
                                });
                            }
                        }
                    });

                    //ui.add_space(ui.available_height() - ui.spacing().interact_size.y * 2.0);
                    ui.add_space(10.0);
                    if ui.button(RichText::new("Confirm").text_style(TextStyle::Heading)).clicked() {
                        ret = true;
                    }
                    ui.add_space(5.0);
                });

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.allocate_space([300.0, 0.0].into());
                if let Some(import_model) = &self.import_window.model {
                    // make this into macro cause i do it a lot
                    // the only things that change are what gets iterated over, and what/how the enum that goes into the selection is made
                    macro_rules! get_selection_status {
                        ($x:expr, |$i:ident| $y:expr) => {{
                            let mut num_banks = 0;
                            let mut selected_banks = 0;
                            for $i in $x.iter().enumerate() {
                                num_banks += 1;
                                if self.import_window.import_selection.contains(&$y) {
                                    selected_banks += 1;
                                }
                            }

                            match (num_banks, selected_banks) {
                                (_, 0) => SelectionType::None,
                                (_, _) if selected_banks < num_banks => SelectionType::Partial,
                                (_, _) if selected_banks == num_banks => SelectionType::Total,
                                _ => unreachable!(),
                            }
                        }};
                    }

                    //Header
                    let selection_status = if self.import_window.import_selection.contains(&TreeValue::Header) {
                        SelectionType::Total
                    } else {
                        SelectionType::None
                    };
                    if selectable_label(ui, selection_status, "Header Geometry Data")
                        .on_hover_text("Bound box, radius, mass and moment of inertia")
                        .clicked()
                    {
                        toggle(&mut self.import_window.import_selection, TreeValue::Header);
                    }

                    // Submodels
                    fn make_submodel_child_list(
                        import_model: &pof::Model, selection: &mut BTreeSet<TreeValue>, smodel: &Submodel, ui: &mut Ui, bother_with_coloring: bool,
                        options: &ImportOptions,
                    ) {
                        let tree_val = TreeValue::Submodels(SubmodelTreeValue::Submodel(smodel.id));
                        let selection_status = {
                            if bother_with_coloring {
                                if selection.contains(&tree_val) {
                                    SelectionType::Total
                                } else {
                                    let mut num_submodels = 0;
                                    let mut selected_submodels = 0;
                                    import_model.do_for_recursive_smodel_children(smodel.id, &mut |smodel| {
                                        num_submodels += 1;
                                        if selection.contains(&TreeValue::Submodels(SubmodelTreeValue::Submodel(smodel.id))) {
                                            selected_submodels += 1;
                                        }
                                    });

                                    match (num_submodels, selected_submodels) {
                                        (_, 0) => SelectionType::None,
                                        (_, _) if selected_submodels < num_submodels => SelectionType::Partial,
                                        (_, _) if selected_submodels == num_submodels => SelectionType::Total,
                                        _ => unreachable!(),
                                    }
                                }
                            } else {
                                SelectionType::None
                            }
                        };

                        if smodel.children().next().is_none() {
                            if selectable_label(ui, selection_status, &smodel.name).clicked() {
                                toggle(selection, tree_val);

                                if options.auto_select_turrets {
                                    for (i, turret) in import_model.turrets.iter().enumerate() {
                                        if turret.base_model == smodel.id {
                                            set(selection, selection.contains(&tree_val), TreeValue::Turrets(TurretTreeValue::Turret(i)));
                                        }
                                    }
                                }
                                if options.auto_select_paths {
                                    for (i, path) in import_model.paths.iter().enumerate() {
                                        if path.parent.to_lowercase() == smodel.name.to_lowercase() {
                                            set(selection, selection.contains(&tree_val), TreeValue::Paths(PathTreeValue::Path(i)));
                                        }
                                    }
                                }
                            }
                        } else {
                            let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new(format!("import {}", tree_val)), false);
                            let header_is_open = state.is_open();
                            state
                                .show_header(ui, |ui| {
                                    if selectable_label(ui, selection_status, &smodel.name).clicked() {
                                        toggle(selection, tree_val);

                                        if options.auto_select_smodel_children {
                                            // a bit confusing, but do_for_recursive_smodel_children will also run on the submodel you call it on
                                            // so save its selection status now, rather than accidentally toggle it back to its old status
                                            let enable_disable = selection.contains(&tree_val);
                                            import_model.do_for_recursive_smodel_children(smodel.id, &mut |smodel| {
                                                set(selection, enable_disable, TreeValue::Submodels(SubmodelTreeValue::Submodel(smodel.id)));
                                            });
                                        }
                                        if options.auto_select_turrets {
                                            for (i, turret) in import_model.turrets.iter().enumerate() {
                                                if turret.base_model == smodel.id {
                                                    set(selection, selection.contains(&tree_val), TreeValue::Turrets(TurretTreeValue::Turret(i)));
                                                }
                                            }
                                        }
                                        if options.auto_select_paths {
                                            for (i, path) in import_model.paths.iter().enumerate() {
                                                if path.parent.to_lowercase() == smodel.name.to_lowercase() {
                                                    set(selection, selection.contains(&tree_val), TreeValue::Paths(PathTreeValue::Path(i)));
                                                }
                                            }
                                        }
                                    }
                                })
                                .body(|ui| {
                                    for &i in smodel.children() {
                                        make_submodel_child_list(import_model, selection, &import_model.submodels[i], ui, header_is_open, options)
                                    }
                                });
                        }
                    }

                    if !import_model.submodels.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Submodels"), false);
                        let header_is_open = state.is_open();
                        let selection_status =
                            get_selection_status!(import_model.submodels, |i| TreeValue::Submodels(SubmodelTreeValue::Submodel(i.1.id)));
                        state
                            .show_header(ui, |ui| {
                                if selectable_label(ui, selection_status, format!("Submodels ({})", import_model.submodels.len())).clicked() {
                                    if num_submodels_selected == import_model.submodels.len() {
                                        for smodel in &import_model.submodels {
                                            self.import_window
                                                .import_selection
                                                .remove(&TreeValue::Submodels(SubmodelTreeValue::Submodel(smodel.id)));
                                        }
                                    } else {
                                        for smodel in &import_model.submodels {
                                            self.import_window
                                                .import_selection
                                                .insert(TreeValue::Submodels(SubmodelTreeValue::Submodel(smodel.id)));
                                        }
                                    }
                                }
                            })
                            .body(|ui| {
                                for smodel in &import_model.submodels {
                                    if smodel.parent().is_none() {
                                        make_submodel_child_list(
                                            import_model,
                                            &mut self.import_window.import_selection,
                                            smodel,
                                            ui,
                                            header_is_open,
                                            &self.import_window.import_options,
                                        );
                                    }
                                }
                            });
                    }

                    macro_rules! header_stuff {
                        (($status:expr, $list:ident, $selected:expr), $text:expr, |$i:ident| $enum:expr) => {{
                            |ui| {
                                if selectable_label(ui, $status, $text).clicked() {
                                    if $selected == import_model.$list.len() {
                                        for $i in import_model.$list.iter().enumerate() {
                                            self.import_window.import_selection.remove(&$enum);
                                        }
                                    } else {
                                        for $i in import_model.$list.iter().enumerate() {
                                            self.import_window.import_selection.insert($enum);
                                        }
                                    }
                                }
                            }
                        }};
                    }

                    macro_rules! body_stuff {
                        ($list:ident, |$bank:ident| $text:expr, |$i:ident| $enum:expr) => {{
                            |ui| {
                                for ($i, $bank) in import_model.$list.iter().enumerate() {
                                    let tree_val = $enum;
                                    let selection_status = if self.import_window.import_selection.contains(&tree_val) {
                                        SelectionType::Total
                                    } else {
                                        SelectionType::None
                                    };
                                    if selectable_label(ui, selection_status, $text).clicked() {
                                        toggle(&mut self.import_window.import_selection, tree_val);
                                    }
                                }
                            }
                        }};
                    }

                    if !import_model.primary_weps.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Primary Weapons"), false);
                        let selection_status =
                            get_selection_status!(import_model.primary_weps, |i| TreeValue::Weapons(WeaponTreeValue::PriBank(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, primary_weps, num_pri_banks_selected),
                                    format!("Primary Banks ({})", import_model.primary_weps.len()),
                                    |i| TreeValue::Weapons(WeaponTreeValue::PriBank(i.0))
                                ),
                            )
                            .body(body_stuff!(
                                primary_weps,
                                |bank| format!("Primary Bank {} ({} point{})", i + 1, bank.len(), if bank.len() == 1 { "" } else { "s" }),
                                |i| TreeValue::Weapons(WeaponTreeValue::PriBank(i))
                            ));
                    }

                    if !import_model.secondary_weps.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Secondary Weapons"), false);
                        let selection_status =
                            get_selection_status!(import_model.secondary_weps, |i| TreeValue::Weapons(WeaponTreeValue::SecBank(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, secondary_weps, num_sec_banks_selected),
                                    format!("Secondary Banks ({})", import_model.secondary_weps.len()),
                                    |i| TreeValue::Weapons(WeaponTreeValue::SecBank(i.0))
                                ),
                            )
                            .body(body_stuff!(
                                secondary_weps,
                                |bank| format!("Secondary Bank {} ({} point{})", i + 1, bank.len(), if bank.len() == 1 { "" } else { "s" }),
                                |i| TreeValue::Weapons(WeaponTreeValue::SecBank(i))
                            ));
                    }

                    if !import_model.docking_bays.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Docking Bay"), false);
                        let selection_status =
                            get_selection_status!(import_model.docking_bays, |i| TreeValue::DockingBays(DockingTreeValue::Bay(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, docking_bays, num_docks_selected),
                                    format!("Docking Bays ({})", import_model.docking_bays.len()),
                                    |i| TreeValue::DockingBays(DockingTreeValue::Bay(i.0))
                                ),
                            )
                            .body(body_stuff!(docking_bays, |bank| format!("{}", bank.get_name().unwrap_or("Unnamed Dock")), |i| {
                                TreeValue::DockingBays(DockingTreeValue::Bay(i))
                            }));
                    }

                    if !import_model.thruster_banks.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Thruster Bank"), false);
                        let selection_status =
                            get_selection_status!(import_model.thruster_banks, |i| TreeValue::Thrusters(ThrusterTreeValue::Bank(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, thruster_banks, num_thruster_banks_selected),
                                    format!("Thruster Banks ({})", import_model.thruster_banks.len()),
                                    |i| TreeValue::Thrusters(ThrusterTreeValue::Bank(i.0))
                                ),
                            )
                            .body(body_stuff!(
                                thruster_banks,
                                |bank| format!(
                                    "Thruster Bank {} ({} point{})",
                                    i + 1,
                                    bank.glows.len(),
                                    if bank.glows.len() == 1 { "" } else { "s" }
                                ),
                                |i| TreeValue::Thrusters(ThrusterTreeValue::Bank(i))
                            ));
                    }

                    if !import_model.glow_banks.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Glow Bank"), false);
                        let selection_status = get_selection_status!(import_model.glow_banks, |i| TreeValue::Glows(GlowTreeValue::Bank(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, glow_banks, num_glow_banks_selected),
                                    format!("Glow Banks ({})", import_model.glow_banks.len()),
                                    |i| TreeValue::Glows(GlowTreeValue::Bank(i.0))
                                ),
                            )
                            .body(body_stuff!(
                                glow_banks,
                                |bank| format!(
                                    "Glow Bank {} ({} point{})",
                                    i + 1,
                                    bank.glow_points.len(),
                                    if bank.glow_points.len() == 1 { "" } else { "s" }
                                ),
                                |i| TreeValue::Glows(GlowTreeValue::Bank(i))
                            ));
                    }

                    if !import_model.special_points.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Special Points"), false);
                        let selection_status =
                            get_selection_status!(import_model.special_points, |i| TreeValue::SpecialPoints(SpecialPointTreeValue::Point(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, special_points, num_spc_points_selected),
                                    format!("Special Points ({})", import_model.special_points.len()),
                                    |i| TreeValue::SpecialPoints(SpecialPointTreeValue::Point(i.0))
                                ),
                            )
                            .body(
                                //body_stuff!(special_points, |point| format!("{}", point.name), |i| TreeValue::SpecialPoints(SpecialPointTreeValue::Point(i)))
                                |ui| {
                                    for (i, point) in import_model.special_points.iter().enumerate() {
                                        let tree_val = TreeValue::SpecialPoints(SpecialPointTreeValue::Point(i));
                                        let selection_status = if self.import_window.import_selection.contains(&tree_val) {
                                            SelectionType::Total
                                        } else {
                                            SelectionType::None
                                        };
                                        if selectable_label(ui, selection_status, format!("{}", point.name)).clicked() {
                                            let selection = &mut self.import_window.import_selection;
                                            toggle(selection, tree_val);

                                            if self.import_window.import_options.auto_select_paths {
                                                for (i, path) in import_model.paths.iter().enumerate() {
                                                    if path.parent == point.name {
                                                        set(selection, selection.contains(&tree_val), TreeValue::Paths(PathTreeValue::Path(i)));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                            );
                    }

                    if !import_model.turrets.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Turrets"), false);
                        let selection_status = get_selection_status!(import_model.turrets, |i| TreeValue::Turrets(TurretTreeValue::Turret(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, turrets, num_turrets_selected),
                                    format!("Turrets ({})", import_model.turrets.len()),
                                    |i| TreeValue::Turrets(TurretTreeValue::Turret(i.0))
                                ),
                            )
                            .body(body_stuff!(turrets, |turret| format!("{}", import_model.submodels[turret.base_model].name), |i| {
                                TreeValue::Turrets(TurretTreeValue::Turret(i))
                            }));
                    }

                    if !import_model.paths.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Paths"), false);
                        let selection_status = get_selection_status!(import_model.paths, |i| TreeValue::Paths(PathTreeValue::Path(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!((selection_status, paths, num_paths_selected), format!("Paths ({})", import_model.paths.len()), |i| {
                                    TreeValue::Paths(PathTreeValue::Path(i.0))
                                }),
                            )
                            .body(body_stuff!(paths, |path| format!("{}", path.name), |i| TreeValue::Paths(PathTreeValue::Path(i))));
                    }

                    if import_model.shield_data.is_some()
                        && selectable_label(
                            ui,
                            if self.import_window.import_selection.contains(&TreeValue::Shield) {
                                SelectionType::Total
                            } else {
                                SelectionType::None
                            },
                            "Shield",
                        )
                        .clicked()
                    {
                        toggle(&mut self.import_window.import_selection, TreeValue::Shield);
                    }

                    if !import_model.eye_points.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Eye points"), false);
                        let selection_status = get_selection_status!(import_model.eye_points, |i| TreeValue::EyePoints(EyeTreeValue::EyePoint(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, eye_points, num_eyes_selected),
                                    format!("Eye points ({})", import_model.eye_points.len()),
                                    |i| TreeValue::EyePoints(EyeTreeValue::EyePoint(i.0))
                                ),
                            )
                            .body(body_stuff!(eye_points, |_point| format!("Eye Point {}", i + 1), |i| TreeValue::EyePoints(
                                EyeTreeValue::EyePoint(i)
                            )));
                    }

                    if !import_model.insignias.is_empty() {
                        let state = CollapsingState::load_with_default_open(ui.ctx(), Id::new("import Insignias"), false);
                        let selection_status =
                            get_selection_status!(import_model.insignias, |i| TreeValue::Insignia(InsigniaTreeValue::Insignia(i.0)));
                        state
                            .show_header(
                                ui,
                                header_stuff!(
                                    (selection_status, insignias, num_insignias_selected),
                                    format!("Insignia ({})", import_model.insignias.len()),
                                    |i| TreeValue::Insignia(InsigniaTreeValue::Insignia(i.0))
                                ),
                            )
                            .body(body_stuff!(insignias, |_insig| format!("Insignia {}", i + 1), |i| TreeValue::Insignia(
                                InsigniaTreeValue::Insignia(i)
                            )));
                    }
                } else {
                    ui.weak("No model selected to import");
                }
            });
        });

        ret
    }
}

fn toggle<T: Ord>(btree: &mut BTreeSet<T>, val: T) {
    if btree.contains(&val) {
        btree.remove(&val);
    } else {
        btree.insert(val);
    }
}

fn set<T: Ord>(btree: &mut BTreeSet<T>, enable: bool, val: T) {
    if enable {
        btree.insert(val);
    } else {
        btree.remove(&val);
    }
}

fn selectable_label(ui: &mut Ui, selection: SelectionType, label: impl Into<WidgetText>) -> Response {
    let mut button = Button::new(label);
    match selection {
        SelectionType::Total => {
            button = button.fill(Color32::from_rgb(0, 92, 128));
            ui.visuals_mut().override_text_color = Some(Color32::from_rgb(173, 202, 233));
        }
        SelectionType::Partial => {
            button = button.fill(ui.style_mut().visuals.widgets.noninteractive.bg_fill);
            button = button.stroke((1.0, Color32::from_rgb(144, 209, 255)));
        }
        SelectionType::None => button = button.fill(ui.style_mut().visuals.widgets.noninteractive.bg_fill),
    }
    let response = ui.add(button);
    ui.visuals_mut().override_text_color = None;

    response
}

impl PofToolsGui {
    pub fn merge_import_model(&mut self) {
        // this does a lot of much quicker std::mem:takes instead of clones
        // this will mangle the import_model so we must take it by value
        let selection = std::mem::take(&mut self.import_window.import_selection);
        let mut import_model = std::mem::take(&mut self.import_window.model).unwrap();

        // where each imported path ended up here, keyed by its index in the model being imported
        // from, so a docking bay's link can be pointed at it once the whole selection has landed
        let mut path_id_map: HashMap<usize, PathId> = HashMap::new();
        // the bays this import placed, by their index in the receiving model. Each still carries the
        // path index it had in the model being imported from, resolved once every path has landed.
        let mut imported_bays: BTreeSet<usize> = BTreeSet::new();

        // turrets, eye points and glow banks all reference submodels by index. They're collected
        // here and settled in a post-operation once every submodel has been installed below, so a
        // name match sees the whole destination and never indexes a submodel list that hasn't grown
        // yet - which is where a match against an item added earlier in the same import would panic.
        let mut imported_turrets = Vec::new();
        let mut imported_eyes = Vec::new();
        let mut imported_glows = Vec::new();

        // make the model id map to translate old model ids to new model ids
        let mut model_id_map = HashMap::new();
        let mut num_submodels = self.model.submodels.len();
        for tree_val in &selection {
            if let TreeValue::Submodels(SubmodelTreeValue::Submodel(id)) = *tree_val {
                let new_id = match self.import_window.import_type {
                    ImportType::Add => {
                        num_submodels += 1;
                        SubmodelId((num_submodels - 1) as u32)
                    }
                    ImportType::MatchAndReplace => {
                        let mut replaced_id = None;
                        for smodel in &self.model.submodels {
                            if smodel.name == import_model.submodels[id].name {
                                replaced_id = Some(smodel.id);
                                break;
                            }
                        }

                        if let Some(replaced_id) = replaced_id {
                            replaced_id
                        } else {
                            //uh oh, just add instead?
                            num_submodels += 1;
                            SubmodelId((num_submodels - 1) as u32)
                        }
                    }
                };
                model_id_map.insert(id, new_id);
            }
        }

        // we're mangling the import_model as we go, so order is VERY IMPORTANT
        // do the things which require the submode; still be intact
        for tree_val in &selection {
            match *tree_val {
                TreeValue::Header => {
                    let header = std::mem::take(&mut import_model.header);

                    self.model.header.bbox = header.bbox;
                    self.model.header.max_radius = header.max_radius;
                    self.model.header.mass = header.mass;
                    self.model.header.moment_of_inertia = header.moment_of_inertia;
                }
                TreeValue::DockingBays(DockingTreeValue::Bay(idx)) => {
                    // requires getting submodels by name -> still have to be intact
                    let mut dock = std::mem::take(&mut import_model.docking_bays[idx]);

                    // dock.path still indexes the model being imported from; it's remapped to
                    // wherever that path landed here once the whole selection is in, below.

                    if let Some(parent_name) = dock.get_parent_smodel() {
                        if import_model
                            .get_model_id_by_name(parent_name)
                            .is_some_and(|id| !model_id_map.contains_key(&id))
                        {
                            // parent submodel was not imported, lose it
                            properties_delete_field(&mut dock.properties, "$parent_submodel");
                        }
                    }

                    // which bay in this model the imported one became, if it became one at all
                    let new_bay = match self.import_window.import_type {
                        ImportType::Add => {
                            self.model.docking_bays.push(dock);
                            Some(self.model.docking_bays.len() - 1)
                        }
                        ImportType::MatchAndReplace => {
                            // find and replace
                            if let Some(name) = dock.get_name() {
                                if let Some(replaced_idx) = self
                                    .model
                                    .docking_bays
                                    .iter()
                                    .position(|replaced_dock| replaced_dock.get_name() == Some(name))
                                {
                                    self.model.docking_bays[replaced_idx] = dock;
                                    Some(replaced_idx)
                                } else {
                                    // fall back, just add it
                                    self.model.docking_bays.push(dock);
                                    Some(self.model.docking_bays.len() - 1)
                                }
                            } else {
                                // a bay with no name has nothing to match against, and is dropped
                                None
                            }
                        }
                    };

                    if let Some(bay_idx) = new_bay {
                        imported_bays.insert(bay_idx);
                    }
                }
                TreeValue::DockingBays(_) => unreachable!(),
                TreeValue::Thrusters(ThrusterTreeValue::Bank(idx)) => {
                    // requires getting submodels by name -> still have to be intact
                    let mut t_bank = std::mem::take(&mut import_model.thruster_banks[idx]);
                    if let Some(subsys_name) = t_bank.get_engine_subsys() {
                        let mut found_a_match = import_model
                            .get_model_id_by_name(subsys_name)
                            .is_some_and(|id| !model_id_map.contains_key(&id));

                        found_a_match |= import_model
                            .special_points
                            .iter()
                            .filter(|spc_point| spc_point.is_subsystem())
                            .any(|spc_point| spc_point.name.strip_prefix('$').unwrap_or(&spc_point.name) == subsys_name);

                        if !found_a_match {
                            // engine subsys was not imported, lose it
                            properties_delete_field(&mut t_bank.properties, "$engine_subsystem");
                        }
                    }

                    match self.import_window.import_type {
                        ImportType::Add => {
                            self.model.thruster_banks.push(t_bank);
                        }
                        ImportType::MatchAndReplace => {
                            // find and replace
                            if let Some(subsys_name) = t_bank.get_engine_subsys() {
                                if let Some(replaced_bank) = self
                                    .model
                                    .thruster_banks
                                    .iter_mut()
                                    .find(|replaced_bank| replaced_bank.get_engine_subsys() == Some(subsys_name))
                                {
                                    *replaced_bank = t_bank;
                                } else {
                                    // fall back, just add it
                                    self.model.thruster_banks.push(t_bank);
                                }
                            }
                        }
                    }
                }
                TreeValue::Thrusters(_) => unreachable!(),
                TreeValue::Glows(GlowTreeValue::Bank(idx)) => {
                    imported_glows.push(std::mem::take(&mut import_model.glow_banks[idx]));
                }
                TreeValue::Glows(_) => unreachable!(),
                TreeValue::Turrets(TurretTreeValue::Turret(idx)) => {
                    imported_turrets.push(std::mem::take(&mut import_model.turrets[idx]));
                }
                TreeValue::Turrets(_) => unreachable!(),
                TreeValue::EyePoints(EyeTreeValue::EyePoint(idx)) => {
                    imported_eyes.push(std::mem::take(&mut import_model.eye_points[idx]));
                }
                TreeValue::EyePoints(_) => unreachable!(),
                TreeValue::Shield => {
                    let shield = std::mem::take(&mut import_model.shield_data);

                    if let Some(data) = shield {
                        self.model.shield_data = Some(data);
                    }
                }
                TreeValue::Insignia(InsigniaTreeValue::Insignia(idx)) => {
                    let insignia = std::mem::take(&mut import_model.insignias[idx]);

                    // uh what if its attached to a detail level the model doesnt have>????

                    self.model.insignias.push(insignia);
                }
                TreeValue::Insignia(_) => unreachable!(),
                TreeValue::VisualCenter => unreachable!(), // should this be importable?
                TreeValue::Comments => unreachable!(),     // should this be importable?
                TreeValue::Submodels(_) => (),
                TreeValue::Weapons(_) => (),
                TreeValue::Textures(_) => unreachable!(),
                _ => (),
            }
        }

        // the imported submodels' names, captured before the install loop below takes them, so the
        // post-operation can still resolve a turret/eye/glow's source submodel to where it landed
        let import_submodel_names: Vec<String> = import_model.submodels.iter().map(|smodel| smodel.name.clone()).collect();

        let old_smodel_len = self.model.submodels.len();
        for tree_val in &selection {
            match *tree_val {
                TreeValue::Submodels(SubmodelTreeValue::Submodel(id)) => {
                    // translate old texure ids to new one
                    let mut tex_id_map = HashMap::new();
                    for (_, poly) in import_model.submodels[id].bsp_data.collision_tree.leaves_mut() {
                        if let Entry::Vacant(e) = tex_id_map.entry(poly.texture) {
                            // see if this texture already exists
                            if let Some(i) =
                                (self.model.textures.iter()).position(|tex_name| import_model.textures[poly.texture.0 as usize] == *tex_name)
                            {
                                e.insert(TextureId(i as u32));
                            } else {
                                // still here, gotta add a slot i guess
                                e.insert(TextureId(self.model.textures.len() as u32));
                                self.model.textures.push(import_model.textures[poly.texture.0 as usize].clone());
                            }
                        }

                        poly.texture = tex_id_map[&poly.texture];
                    }

                    let import_submodel = &mut import_model.submodels[id];
                    import_submodel.id = model_id_map[&id];

                    // make da swap (or addition)
                    if import_submodel.id.0 as usize >= old_smodel_len {
                        if let Some(parent_id) = import_submodel.parent {
                            import_submodel.parent = model_id_map.get(&parent_id).copied();
                        }

                        self.model.submodels.push(std::mem::take(import_submodel));
                        self.model.header.num_submodels += 1;
                    } else {
                        let target_submodel = &mut self.model.submodels[import_submodel.id];
                        // if you're replacing a submodel, you inherit the parent of what you've replaced
                        import_submodel.parent = target_submodel.parent;

                        // also inherit position
                        import_submodel.offset = target_submodel.offset;

                        *target_submodel = std::mem::take(import_submodel);
                    }
                }
                TreeValue::Weapons(WeaponTreeValue::PriBank(idx)) => {
                    self.model.primary_weps.push(std::mem::take(&mut import_model.primary_weps[idx]))
                }
                TreeValue::Weapons(WeaponTreeValue::SecBank(idx)) => {
                    self.model.secondary_weps.push(std::mem::take(&mut import_model.secondary_weps[idx]))
                }
                TreeValue::SpecialPoints(SpecialPointTreeValue::Point(idx)) => {
                    let spc_point = std::mem::take(&mut import_model.special_points[idx]);

                    match self.import_window.import_type {
                        ImportType::Add => {
                            self.model.special_points.push(spc_point);
                        }
                        ImportType::MatchAndReplace => {
                            if let Some(replaced_point) = self
                                .model
                                .special_points
                                .iter_mut()
                                .find(|replaced_point| replaced_point.name == spc_point.name)
                            {
                                *replaced_point = spc_point;
                            } else {
                                // fall back, just add it
                                self.model.special_points.push(spc_point);
                            }
                        }
                    }
                }
                TreeValue::SpecialPoints(_) => unreachable!(),
                TreeValue::Paths(PathTreeValue::Path(idx)) => {
                    let path = std::mem::take(&mut import_model.paths[idx]);

                    // where it landed, so a docking bay which linked to it can be pointed there
                    let new_path = match self.import_window.import_type {
                        ImportType::Add => {
                            self.model.paths.push(path);
                            self.model.paths.len() - 1
                        }
                        ImportType::MatchAndReplace => {
                            if let Some(replaced_idx) = self.model.paths.iter().position(|replaced_path| replaced_path.name == path.name) {
                                self.model.paths[replaced_idx] = path;
                                replaced_idx
                            } else {
                                // fall back, just add it
                                self.model.paths.push(path);
                                self.model.paths.len() - 1
                            }
                        }
                    };
                    path_id_map.insert(idx, PathId(new_path as u32));
                }
                TreeValue::Paths(_) => unreachable!(),
                _ => (),
            }
        }

        // now every submodel is installed, so the collected turrets, eye points and glow banks can
        // be settled against the finished destination. Each resolves by the name of the submodel it
        // named in the source, so imported and pre-existing submodels are treated the same, and no
        // match indexes a submodel list that isn't there yet.

        // turrets: find the destination base and gun submodels by name (single- or multi-part)
        for mut turret in imported_turrets {
            let gun_name = import_submodel_names.get(turret.gun_model.0 as usize);
            let base_name = import_submodel_names.get(turret.base_model.0 as usize);
            let mut resolved = None;
            for smodel in &self.model.submodels {
                let singlepart_valid = Some(&smodel.name) == gun_name && turret.gun_model == turret.base_model;
                let multipart_valid =
                    Some(&smodel.name) == gun_name && smodel.parent().map(|id| &self.model.submodels[id].name) == base_name;
                if singlepart_valid {
                    resolved = Some((smodel.id, smodel.id));
                } else if multipart_valid {
                    resolved = Some((smodel.parent().unwrap(), smodel.id));
                }
            }
            let (base_model, gun_model) = match resolved {
                Some(ids) => ids,
                None => continue, // no base/gun here to sit on, so lose it
            };
            turret.base_model = base_model;
            turret.gun_model = gun_model;
            match self.import_window.import_type {
                ImportType::Add => self.model.turrets.push(turret),
                ImportType::MatchAndReplace => {
                    if let Some(replaced_turret) = self.model.turrets.iter_mut().find(|replaced_turret| replaced_turret.base_model == base_model)
                    {
                        *replaced_turret = turret;
                    } else {
                        self.model.turrets.push(turret);
                    }
                }
            }
        }

        // eye points: attach to the submodel with the same name, or to nothing if there is none
        for mut point in imported_eyes {
            let attached = point
                .attached_submodel
                .and_then(|id| import_submodel_names.get(id.0 as usize))
                .and_then(|name| self.model.submodels.iter().find(|smodel| smodel.name == *name))
                .map(|smodel| smodel.id);
            point.attached_submodel = attached;
            match self.import_window.import_type {
                ImportType::Add => self.model.eye_points.push(point),
                ImportType::MatchAndReplace => {
                    // replace the existing eye on the same submodel - an unattached one (None)
                    // matching another unattached one - else add it
                    if let Some(replaced_point) = self.model.eye_points.iter_mut().find(|p| p.attached_submodel == attached) {
                        *replaced_point = point;
                    } else {
                        self.model.eye_points.push(point);
                    }
                }
            }
        }

        // glow banks: a bank always sits on a submodel, so a parent that resolves to no same-named
        // submodel falls back to detail0 - or is dropped if there are no submodels at all here
        for mut g_bank in imported_glows {
            let parent = import_submodel_names
                .get(g_bank.model_parent.0 as usize)
                .and_then(|name| self.model.submodels.iter().find(|smodel| smodel.name == *name))
                .map(|smodel| smodel.id);
            if let Some(parent) = parent {
                g_bank.model_parent = parent;
            } else if self.model.submodels.is_empty() {
                continue; // nothing to attach it to, so drop it
            } else {
                g_bank.model_parent = self.model.glow_bank_parent_fallback();
            }
            self.model.glow_banks.push(g_bank);
        }

        // now every selected path has landed, so each imported bay can follow the path it carried
        // over to wherever it ended up - or lose the link if that path didn't come along, rather
        // than keep an index which names a stranger's path here
        for bay_idx in imported_bays {
            let old_path = self.model.docking_bays[bay_idx].path;
            self.model.docking_bays[bay_idx].path = old_path.and_then(|p| path_id_map.get(&(p.0 as usize)).copied());
        }

        self.model.recalc_semantic_name_links();
        self.model.recalc_all_children_ids();

        // the remaps above aim each imported index at where it landed; this is the safety net,
        // establishing the same no-dangling-index invariant the loaders do so a stray one can't
        // reach the UI and panic when it's drawn or selected. As at load, it runs after the recalcs
        // so it sees the model in its final shape (recalc_all_children_ids in particular).
        self.model.sanitize_index_references();
    }
}
