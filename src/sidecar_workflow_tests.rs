#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::sidecar_workflow;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("mm-{name}-{nanos}"));
        dir
    }

    #[test]
    fn dry_run_apply_and_rollback_cycle() {
        let root = unique_temp_dir("workflow");
        let media_dir = root.join("library");
        let state_dir = root.join("state");
        fs::create_dir_all(&media_dir).expect("create media dir");

        let media = media_dir.join("movie.mkv");
        fs::write(&media, b"x").expect("write media file");

        let dry_run = sidecar_workflow::build_plan(&media, "movie-1").expect("build plan");
        let applied = sidecar_workflow::apply_plan(&media, "movie-1", &dry_run.plan_hash, &state_dir)
            .expect("apply plan");

        let sidecar_content = fs::read_to_string(media_dir.join(".mm.json")).expect("read sidecar");
        assert!(sidecar_content.contains("movie-1"));

        let rollback = sidecar_workflow::rollback_operation(&applied.operation_id, &state_dir)
            .expect("rollback operation");
        assert!(rollback.restored);

        assert!(!media_dir.join(".mm.json").exists());

        fs::remove_dir_all(root).expect("cleanup test directory");
    }

    #[test]
    fn updates_item_uid_for_existing_sidecar() {
        let root = unique_temp_dir("workflow-update");
        let media_dir = root.join("library");
        let state_dir = root.join("state");
        fs::create_dir_all(&media_dir).expect("create media dir");

        let media = media_dir.join("movie.mkv");
        fs::write(&media, b"x").expect("write media file");

        let first_plan = sidecar_workflow::build_plan(&media, "movie-1").expect("build first plan");
        sidecar_workflow::apply_plan(&media, "movie-1", &first_plan.plan_hash, &state_dir)
            .expect("apply first plan");

        let second_plan = sidecar_workflow::build_plan(&media, "movie-2").expect("build second plan");
        assert!(matches!(second_plan.action, sidecar_workflow::SidecarPlanAction::Update));
        assert_eq!(second_plan.next_state.item_uid, "movie-2");

        let applied = sidecar_workflow::apply_plan(&media, "movie-2", &second_plan.plan_hash, &state_dir)
            .expect("apply second plan");
        assert_eq!(applied.applied_state.item_uid, "movie-2");

        let sidecar_content = fs::read_to_string(media_dir.join(".mm.json")).expect("read sidecar");
        assert!(sidecar_content.contains("movie-2"));

        fs::remove_dir_all(root).expect("cleanup test directory");
    }
}
