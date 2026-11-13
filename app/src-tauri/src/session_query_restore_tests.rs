use super::{set_workspace_hidden, visible_workspaces, workspace_key};

#[test]
fn 撤销移除后工作区立刻回到列表() {
    let mut hidden = std::collections::BTreeMap::new();
    let rows = || vec![("C:/w/old".to_string(), 90), ("C:/w/new".to_string(), 80)];
    set_workspace_hidden(&mut hidden, workspace_key("C:/w/old"), true);
    assert_eq!(
        visible_workspaces(rows(), &hidden, 10),
        vec!["C:/w/new".to_string()]
    );
    set_workspace_hidden(&mut hidden, workspace_key("C:/w/old"), false);
    assert!(hidden.is_empty());
    assert_eq!(visible_workspaces(rows(), &hidden, 10).len(), 2);
}
