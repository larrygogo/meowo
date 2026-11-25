use super::*;

fn dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("team");
    (tmp, d)
}

fn args(dto: &TeamPersonalConfigDto) -> SaveTeamPersonalConfigArgs {
    SaveTeamPersonalConfigArgs {
        token: dto.token.clone(),
        taste: dto.taste.clone(),
        protected: dto.protected.clone(),
        do_and_report: dto.do_and_report.clone(),
        ask_at_end: dto.ask_at_end.clone(),
        ask_first: dto.ask_first.clone(),
    }
}

#[test]
fn missing_config_reads_as_empty_and_is_created_on_first_save() {
    let (_t, d) = dir();
    let dto = load_from(&d).unwrap();
    assert!(!dto.exists && dto.error.is_none() && dto.token.is_empty());
    assert!(!d.exists(), "只读不建目录");
    let mut a = args(&dto);
    a.protected = vec!["acme-*".into(), " acme-* ".into(), "".into()];
    a.ask_first = vec!["改数据库结构".into()];
    let saved = save_to(&d, &a).unwrap();
    assert!(saved.exists);
    assert_eq!(saved.protected, vec!["acme-*"], "去空白、去重、去空");
    let text = std::fs::read_to_string(d.join("config.json")).unwrap();
    assert!(text.contains("_说明"));
    // 守卫读到的是同一份
    let guard = meowo_reporter::team_guard::Config::load(Some(&d));
    assert_eq!(guard.protected, vec!["acme-*"]);
    assert!(guard.broken.is_none());
}

/// 界面只改自己管的字段：额外拦截的命令、用户自己加的键、说明都要原样留着。
#[test]
fn unknown_fields_and_blocked_commands_survive_a_save() {
    let (_t, d) = dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("config.json"),
        r#"{"_说明":"我的","taste":"D:/notes/taste.md","protected":["old-*"],"blocked_commands":[{"pattern":"notify\\.py","what":"发通知"}],"mine":{"x":1},"authority":{"ask_at_end":["合并"],"note":"留着"}}"#,
    )
    .unwrap();
    let dto = load_from(&d).unwrap();
    assert_eq!(dto.blocked_command_count, 1);
    assert_eq!(dto.taste.as_deref(), Some("D:/notes/taste.md"));
    assert!(!dto.taste_exists);
    let mut a = args(&dto);
    a.protected.push("new-repo".into());
    a.taste = None;
    save_to(&d, &a).unwrap();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(d.join("config.json")).unwrap()).unwrap();
    assert_eq!(v["_说明"], "我的");
    assert_eq!(v["mine"]["x"], 1);
    assert_eq!(v["blocked_commands"][0]["what"], "发通知");
    assert_eq!(v["authority"]["note"], "留着");
    assert_eq!(v["protected"], json!(["old-*", "new-repo"]));
    assert!(v.get("taste").is_none(), "清空品味清单路径就删掉这个键");
}

/// 写坏了：守卫和界面同一个判断，界面不能保存覆盖。
#[test]
fn a_broken_file_is_reported_and_never_overwritten() {
    let (_t, d) = dir();
    std::fs::create_dir_all(&d).unwrap();
    let broken = "{\"protected\": [\"acme-*\",],}";
    std::fs::write(d.join("config.json"), broken).unwrap();
    let dto = load_from(&d).unwrap();
    assert!(dto.exists);
    assert!(dto.error.as_deref().is_some_and(|e| e.contains("JSON")), "{:?}", dto.error);
    assert_eq!(save_to(&d, &args(&dto)).unwrap_err(), "team/config_broken");
    assert_eq!(create_taste_in(&d, &dto.token).unwrap_err(), "team/config_broken");
    assert_eq!(std::fs::read_to_string(d.join("config.json")).unwrap(), broken);
}

/// 子页打开期间用户在编辑器里改了文件：保存不能把那次修改冲掉。
#[test]
fn an_outside_edit_since_loading_blocks_the_save() {
    let (_t, d) = dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("config.json"), r#"{"protected":["a"]}"#).unwrap();
    let dto = load_from(&d).unwrap();
    std::fs::write(d.join("config.json"), r#"{"protected":["a","b"]}"#).unwrap();
    assert_eq!(save_to(&d, &args(&dto)).unwrap_err(), "team/config_changed");
    assert_eq!(load_from(&d).unwrap().protected, vec!["a", "b"]);
    // 文件被删了也算变化
    let dto = load_from(&d).unwrap();
    std::fs::remove_file(d.join("config.json")).unwrap();
    assert_eq!(save_to(&d, &args(&dto)).unwrap_err(), "team/config_changed");
}

#[test]
fn floor_items_written_into_looser_tiers_are_flagged() {
    let (_t, d) = dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("config.json"),
        r#"{"authority":{"do_and_report":["推 beta","对外发消息（飞书群）"],"ask_at_end":["上生产","合并"],"ask_first":["删除数据"]}}"#,
    )
    .unwrap();
    let dto = load_from(&d).unwrap();
    assert_eq!(dto.floor_misplaced, vec!["对外发消息（飞书群）", "上生产"]);
    std::fs::write(d.join("config.json"), r#"{"authority":{"do_and_report":["Deploy to production"]}}"#).unwrap();
    assert_eq!(load_from(&d).unwrap().floor_misplaced, vec!["Deploy to production"]);
}

#[test]
fn creating_a_taste_list_points_the_config_at_it_without_clobbering() {
    let (_t, d) = dir();
    let dto = create_taste_in(&d, "").unwrap();
    let taste = d.join("taste.md");
    assert_eq!(dto.taste.as_deref(), Some(taste.to_string_lossy().as_ref()));
    assert!(dto.taste_exists);
    assert!(std::fs::read_to_string(&taste).unwrap().contains("品味清单"));
    // 已有自己写的 taste.md：不覆盖
    std::fs::write(&taste, "我的").unwrap();
    let again = create_taste_in(&d, &dto.token).unwrap();
    assert_eq!(std::fs::read_to_string(&taste).unwrap(), "我的");
    assert!(again.taste_exists);
    assert_eq!(create_taste_in(&d, "stale").unwrap_err(), "team/config_changed");
}

#[test]
fn missing_authority_falls_back_to_the_template_defaults() {
    let (_t, d) = dir();
    let dto = load_from(&d).unwrap();
    assert!(dto.authority_defaulted);
    assert_eq!(dto.do_and_report.len(), 4);
    assert_eq!(dto.ask_at_end, vec!["推 beta / 部署到测试环境（改动攒着，结尾一起问）", "合并到主分支", "发版"]);
    assert!(dto.ask_first.is_empty());
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("config.json"), r#"{"protected":[]}"#).unwrap();
    assert!(load_from(&d).unwrap().authority_defaulted);
    std::fs::write(d.join("config.json"), r#"{"authority":{"do_and_report":[]}}"#).unwrap();
    let dto = load_from(&d).unwrap();
    assert!(!dto.authority_defaulted);
    assert!(dto.do_and_report.is_empty(), "写了空档就是空档，不用默认值");
    assert_eq!(dto.ask_at_end.len(), 3, "没写的那一档用默认值");
}

#[test]
fn floor_synonyms_in_ask_first_merge_into_the_locked_group() {
    let (_t, d) = dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("config.json"), r#"{"authority":{"ask_first":["上生产","花钱：开云主机、买资源","改数据库结构"]}}"#).unwrap();
    let dto = load_from(&d).unwrap();
    assert_eq!(dto.ask_first, vec!["改数据库结构"]);
    assert!(dto.floor_misplaced.is_empty());
}

#[test]
fn a_broken_file_reports_the_line_and_its_neighbours() {
    let (_t, d) = dir();
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("config.json"), "{\n  \"protected\": [\n    \"a\"\n    \"b\"\n  ]\n}\n").unwrap();
    let dto = load_from(&d).unwrap();
    assert_eq!(dto.error_line, Some(4));
    assert_eq!(dto.error_first_line, 2);
    assert_eq!(dto.error_lines, vec!["  \"protected\": [", "    \"a\"", "    \"b\"", "  ]"]);
}
