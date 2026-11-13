//! 必须独立测试进程显式运行；认证只进临时home，普通cargo test不会联网或拉起模型。

use super::*;
use meowo_protocol::ipc::LeadGateAnswerDto;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::Read;

const MAX_FILE: usize = 2 * 1024 * 1024;
const APPROVAL_WAIT: Duration = Duration::from_secs(300);

struct Environment {
    old: Vec<(String, Option<OsString>)>,
    cwd: PathBuf,
}
impl Environment {
    fn new() -> Self {
        Self {
            old: vec![],
            cwd: std::env::current_dir().unwrap(),
        }
    }
    fn set(&mut self, key: &str, value: Option<&std::ffi::OsStr>) {
        if !self.old.iter().any(|(k, _)| k == key) {
            self.old.push((key.into(), std::env::var_os(key)));
        }
        if let Some(value) = value {
            std::env::set_var(key, value)
        } else {
            std::env::remove_var(key)
        }
    }
    fn path(&mut self, key: &str, path: &Path) {
        self.set(key, Some(path.as_os_str()));
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in self.old.iter().rev() {
            if let Some(value) = value {
                std::env::set_var(key, value)
            } else {
                std::env::remove_var(key)
            }
        }
        let _ = std::env::set_current_dir(&self.cwd);
    }
}

fn small(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("probe/source_read_failed".into()),
    };
    let mut bytes = vec![];
    file.take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "probe/source_read_failed")?;
    if bytes.len() > MAX_FILE {
        return Err("probe/source_too_large".into());
    }
    Ok(Some(bytes))
}
fn source_json(path: &Path) -> Result<Option<Value>, String> {
    small(path)?
        .map(|bytes| {
            let raw = String::from_utf8(bytes).map_err(|_| "probe/source_config_invalid")?;
            serde_json::from_str(&crate::terminal::strip_jsonc_comments(&raw))
                .map_err(|_| "probe/source_config_invalid".into())
        })
        .transpose()
}
fn source_toml(path: &Path) -> Result<Option<toml_edit::DocumentMut>, String> {
    small(path)?
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| "probe/source_config_invalid")?
                .parse()
                .map_err(|_| "probe/source_config_invalid".into())
        })
        .transpose()
}
fn kimi_clone_config(doc: &toml_edit::DocumentMut) -> toml_edit::DocumentMut {
    let mut out = toml_edit::DocumentMut::new();
    for key in ["default_model", "default_thinking"] {
        if let Some(v) = doc.get(key) {
            out[key] = fresh_toml_item(v);
        }
    }
    if let Some(model) = doc.get("default_model").and_then(toml_edit::Item::as_str) {
        if let Some(v) = doc.get("models").and_then(|v| v.get(model)) {
            let mut models = toml_edit::Table::new();
            models.insert(model, fresh_toml_item(v));
            out.insert("models", toml_edit::Item::Table(models));
            if let Some(provider) = v.get("provider").and_then(toml_edit::Item::as_str) {
                if let Some(v) = doc.get("providers").and_then(|p| p.get(provider)) {
                    let mut providers = toml_edit::Table::new();
                    providers.insert(provider, fresh_toml_item(v));
                    out.insert("providers", toml_edit::Item::Table(providers));
                }
            }
        }
    }
    out
}
fn fresh_toml_item(item: &toml_edit::Item) -> toml_edit::Item {
    // 真实Kimi探针与回归均证实：克隆原Table会保留位置/implicit元数据，新Document内存有表但序列化丢表。
    // 这里只重建白名单配置的语义结构；不带原文格式、注释，也不修改来源配置。
    match item {
        toml_edit::Item::None => toml_edit::Item::None,
        toml_edit::Item::Value(value) => toml_edit::Item::Value(fresh_toml_value(value)),
        toml_edit::Item::Table(table) => {
            let mut fresh = toml_edit::Table::new();
            for (key, item) in table.iter() {
                fresh.insert(key, fresh_toml_item(item));
            }
            toml_edit::Item::Table(fresh)
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            let mut fresh = toml_edit::ArrayOfTables::new();
            for table in tables.iter() {
                let mut entry = toml_edit::Table::new();
                for (key, item) in table.iter() {
                    entry.insert(key, fresh_toml_item(item));
                }
                fresh.push(entry);
            }
            toml_edit::Item::ArrayOfTables(fresh)
        }
    }
}
fn fresh_toml_value(value: &toml_edit::Value) -> toml_edit::Value {
    use toml_edit::Value as Toml;
    match value {
        Toml::String(value) => Toml::from(value.value().clone()),
        Toml::Integer(value) => Toml::from(*value.value()),
        Toml::Float(value) => Toml::from(*value.value()),
        Toml::Boolean(value) => Toml::from(*value.value()),
        Toml::Datetime(value) => Toml::Datetime(toml_edit::Formatted::new(*value.value())),
        Toml::Array(values) => {
            let mut fresh = toml_edit::Array::new();
            for value in values.iter() {
                fresh.push(fresh_toml_value(value));
            }
            Toml::Array(fresh)
        }
        Toml::InlineTable(table) => {
            let mut fresh = toml_edit::InlineTable::new();
            for (key, value) in table.iter() {
                fresh.insert(key, fresh_toml_value(value));
            }
            Toml::InlineTable(fresh)
        }
    }
}
fn kimi_config_shape(doc: &toml_edit::DocumentMut) -> Value {
    let keys = |item: Option<&toml_edit::Item>| {
        item.and_then(toml_edit::Item::as_table_like)
            .map(|table| {
                table
                    .iter()
                    .map(|(key, _)| key.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let selected = doc.get("default_model").and_then(toml_edit::Item::as_str);
    let model = selected.and_then(|id| doc.get("models").and_then(|table| table.get(id)));
    let provider = model
        .and_then(|model| model.get("provider"))
        .and_then(toml_edit::Item::as_str)
        .and_then(|id| doc.get("providers").and_then(|table| table.get(id)));
    json!({"rootKeys":doc.iter().map(|(key,_)|key).collect::<Vec<_>>(),"selectedModelPresent":selected.is_some(),"selectedModelTablePresent":model.is_some(),"selectedModelKeys":keys(model),"referencedProviderPresent":provider.is_some(),"referencedProviderKeys":keys(provider),"providerInlineKeyPresent":provider.and_then(|p|p.get("api_key")).and_then(toml_edit::Item::as_str).is_some_and(|v|!v.is_empty()),"providerBaseUrlPresent":provider.and_then(|p|p.get("base_url")).and_then(toml_edit::Item::as_str).is_some_and(|v|!v.is_empty())})
}
fn strings(value: &Value, secret: &mut Vec<String>) {
    match value {
        Value::String(s) if s.len() >= 8 => secret.push(s.clone()),
        Value::Array(a) => {
            for v in a {
                strings(v, secret)
            }
        }
        Value::Object(o) => {
            for v in o.values() {
                strings(v, secret)
            }
        }
        _ => {}
    }
}
fn toml_secrets(item: &toml_edit::Item, secrets: &mut Vec<String>) {
    if let Some(table) = item.as_table_like() {
        for (key, item) in table.iter() {
            let key = key.to_ascii_lowercase().replace('_', "");
            if ["key", "token", "secret", "password", "authorization"]
                .iter()
                .any(|s| key.contains(s))
            {
                if let Some(s) = item.as_str().filter(|s| s.len() >= 8) {
                    secrets.push(s.into());
                }
            }
            toml_secrets(item, secrets);
        }
    }
}

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    db: PathBuf,
    provider: String,
    copied: Vec<String>,
    secrets: Vec<String>,
    id: Option<i64>,
    environment: Option<Environment>,
}
impl Sandbox {
    fn new(provider: &str) -> Result<Self, String> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/lead-provider-probes/manager-providers")
            .join(format!("{provider}-{}", crate::remote::generate_token()?));
        let home = root.join("private-home");
        let cwd = root.join("cwd");
        std::fs::create_dir_all(&home).map_err(|_| "probe/prepare_failed")?;
        std::fs::create_dir_all(&cwd).map_err(|_| "probe/prepare_failed")?;
        let root = dunce::canonicalize(root).map_err(|_| "probe/prepare_failed")?;
        Ok(Self {
            home: root.join("private-home"),
            cwd: root.join("cwd"),
            db: root.join("board.db"),
            root,
            provider: provider.into(),
            copied: vec![],
            secrets: vec![],
            id: None,
            environment: None,
        })
    }
    fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<(), String> {
        let target = self.home.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "probe/prepare_failed")?;
        }
        std::fs::write(target, bytes).map_err(|_| "probe/prepare_failed")?;
        self.copied.push(relative.into());
        Ok(())
    }
    fn copy_json(&mut self, source: &Path, relative: &str) -> Result<(), String> {
        if let Some(value) = source_json(source)? {
            strings(&value, &mut self.secrets);
            self.write(
                relative,
                &serde_json::to_vec(&value).map_err(|_| "probe/source_config_invalid")?,
            )?;
        }
        Ok(())
    }
    fn prepare(&mut self) -> Result<Value, String> {
        let source_home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .ok_or("probe/source_home_missing")?;
        let plugin = meowo_agent::by_id(&self.provider).ok_or("probe/provider_invalid")?;
        let install = plugin.resolve().ok_or("probe/source_cli_missing")?;
        let native_argv = install
            .launch
            .clone()
            .unwrap_or_else(|| plugin.launch_argv());
        let source = install.data_dir.clone();
        match self.provider.as_str() {
            "codex" => {
                self.copy_json(&source.join("auth.json"), ".codex/auth.json")?;
                if let Some(doc) = source_toml(&source.join("config.toml"))? {
                    let mut out = toml_edit::DocumentMut::new();
                    for key in ["model", "model_provider", "model_reasoning_effort"] {
                        if let Some(v) = doc.get(key) {
                            out[key] = v.clone();
                        }
                    }
                    let provider = doc
                        .get("model_provider")
                        .and_then(toml_edit::Item::as_str)
                        .unwrap_or("openai");
                    if let Some(v) = doc.get("model_providers").and_then(|v| v.get(provider)) {
                        out["model_providers"][provider] = v.clone();
                    }
                    toml_secrets(out.as_item(), &mut self.secrets);
                    self.write(".codex/config.toml", out.to_string().as_bytes())?;
                }
            }
            "kimi" => {
                self.copy_json(
                    &source.join("credentials/kimi-code.json"),
                    ".kimi-code/credentials/kimi-code.json",
                )?;
                if let Some(doc) = source_toml(&source.join("config.toml"))? {
                    let out = kimi_clone_config(&doc);
                    let reparsed: toml_edit::DocumentMut = out
                        .to_string()
                        .parse()
                        .map_err(|_| "probe/clone_config_invalid")?;
                    self.record("config-shape.json", &json!({"sourceVariant":install.variant_tag,"sourceDataDir":source,"source":kimi_config_shape(&doc),"cloneBeforeSerialize":kimi_config_shape(&out),"cloneAfterReparse":kimi_config_shape(&reparsed),"containsSecretValues":false}))?;
                    if kimi_config_shape(&out) != kimi_config_shape(&reparsed) {
                        return Err("probe/clone_config_invalid".into());
                    }
                    toml_secrets(out.as_item(), &mut self.secrets);
                    self.write(".kimi-code/config.toml", out.to_string().as_bytes())?;
                }
            }
            "gemini" => {
                let settings =
                    source_json(&source.join("settings.json"))?.unwrap_or_else(|| json!({}));
                let selected = settings
                    .pointer("/security/auth/selectedType")
                    .and_then(Value::as_str)
                    .unwrap_or("oauth-personal");
                let credentials = source.join("oauth_creds.json");
                let oauth_credentials_present = credentials.is_file();
                let mut copied_environment_keys = vec![];
                let mut api_key_present = false;
                if selected == "oauth-personal" {
                    if !oauth_credentials_present {
                        // Gemini 0.65 的个人OAuth只认这个文件；空home继续启动只会无事件超时，
                        // 既浪费探针时间，又会把“未登录”误记成ACP兼容性故障。
                        self.record("gemini-auth-preflight.json", &json!({"provider":"gemini","selectedAuth":selected,"oauthCredentialsPresent":false,"copiedEnvironmentKeys":[],"apiKeyPresent":false,"containsSecretValues":false}))?;
                        return Err("probe/gemini_auth_unavailable".into());
                    }
                    self.copy_json(&credentials, ".gemini/oauth_creds.json")?;
                }
                let mut out = json!({"security":{"auth":{"selectedType":selected}}});
                if let Some(model) = settings.pointer("/model/name").and_then(Value::as_str) {
                    out["model"] = json!({"name":model});
                }
                self.write(
                    ".gemini/settings.json",
                    &serde_json::to_vec(&out).map_err(|_| "probe/source_config_invalid")?,
                )?;
                // Gemini 会在首个非交互请求前询问工作区信任；探针创建的git目录并非用户项目，
                // 将“只信任这一个临时目录”的规则写入私有home，才能检验模型/ACP而非交互确认。
                self.write(
                    ".gemini/trustedFolders.json",
                    &serde_json::to_vec(&json!({
                        self.cwd.to_string_lossy(): "TRUST_FOLDER"
                    }))
                    .map_err(|_| "probe/source_config_invalid")?,
                )?;
                if let Some(bytes) = small(&source.join(".env"))? {
                    let raw =
                        String::from_utf8(bytes).map_err(|_| "probe/source_config_invalid")?;
                    let lines = raw
                        .lines()
                        .filter(|line| {
                            line.split_once('=').is_some_and(|(k, _)| {
                                [
                                    "GEMINI_API_KEY",
                                    "GOOGLE_API_KEY",
                                    "GOOGLE_GENAI_USE_VERTEXAI",
                                    "GOOGLE_CLOUD_PROJECT",
                                    "GOOGLE_CLOUD_LOCATION",
                                ]
                                .contains(&k.trim().trim_start_matches("export "))
                            })
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    for line in lines.lines() {
                        if let Some((key, value)) = line.split_once('=') {
                            let key = key.trim().trim_start_matches("export ");
                            copied_environment_keys.push(key.to_owned());
                            if key.contains("KEY") {
                                let value = value.trim().trim_matches(['\'', '"']);
                                api_key_present |= !value.is_empty();
                                self.secrets.push(value.into());
                            }
                        }
                    }
                    // 未信任项目的Gemini会跳过HOME/.gemini/.env；HOME/.env是已核实的认证查找路径。
                    self.write(".env", lines.as_bytes())?;
                }
                self.record("gemini-auth-preflight.json", &json!({"provider":"gemini","selectedAuth":selected,"oauthCredentialsPresent":oauth_credentials_present,"copiedEnvironmentKeys":copied_environment_keys,"apiKeyPresent":api_key_present,"containsSecretValues":false}))?;
                if selected == "gemini-api-key" && !api_key_present {
                    return Err("probe/gemini_auth_unavailable".into());
                }
            }
            "opencode" => {
                let config = source_json(&source.join("opencode.json"))?
                    .or(source_json(&source.join("opencode.jsonc"))?)
                    .unwrap_or_else(|| json!({}));
                let data = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| source_home.join(".local/share"));
                let auth =
                    source_json(&data.join("opencode/auth.json"))?.unwrap_or_else(|| json!({}));
                let model_provider = config["model"]
                    .as_str()
                    .and_then(|model| model.split_once('/'))
                    .map(|(provider, _)| provider.to_owned());
                let configured = std::env::var("MEOWO_PROBE_OPENCODE_PROVIDER").ok();
                let selected = configured
                    .as_ref()
                    .filter(|provider| auth.get(*provider).is_some())
                    .cloned()
                    .or_else(|| {
                        model_provider
                            .as_ref()
                            .filter(|provider| auth.get(*provider).is_some())
                            .cloned()
                    })
                    // 用户的默认模型可能属于已登出 provider；隔离探针若仍优先它，
                    // 会漏掉唯一可安全复制的凭据，误报为协议投递失败。
                    .or_else(|| {
                        auth.as_object()
                            .filter(|providers| providers.len() == 1)
                            .and_then(|providers| providers.keys().next().cloned())
                    })
                    .or(configured)
                    .or(model_provider.clone());
                if selected.is_none() && auth.as_object().is_some_and(|a| a.len() > 1) {
                    return Err("probe/opencode_provider_selection_required".into());
                }
                let mut out = json!({});
                for key in ["model", "small_model"] {
                    let matches_selected = config[key]
                        .as_str()
                        .and_then(|model| model.split_once('/'))
                        .is_none_or(|(provider, _)| selected.as_deref() == Some(provider));
                    if matches_selected {
                        if let Some(v) = config.get(key) {
                            out[key] = v.clone();
                        }
                    }
                }
                if let Some(provider) = selected {
                    if let Some(entry) = auth.get(&provider) {
                        let one = json!({provider.clone():entry});
                        strings(&one, &mut self.secrets);
                        self.write(
                            "xdg-data/opencode/auth.json",
                            &serde_json::to_vec(&one).map_err(|_| "probe/source_config_invalid")?,
                        )?;
                    }
                    if let Some(entry) = config.get("provider").and_then(|p| p.get(&provider)) {
                        strings(
                            entry.get("options").unwrap_or(&Value::Null),
                            &mut self.secrets,
                        );
                        out["provider"] = json!({provider:entry});
                    }
                }
                self.write(
                    "opencode-config/opencode.json",
                    &serde_json::to_vec(&out).map_err(|_| "probe/source_config_invalid")?,
                )?;
            }
            _ => return Err("probe/provider_invalid".into()),
        }
        let mut environment = Environment::new();
        for (key, value) in std::env::vars() {
            if key.starts_with("MEOWO_") {
                environment.set(&key, None);
            }
            if ["key", "token", "secret", "password", "auth"]
                .iter()
                .any(|part| key.to_ascii_lowercase().contains(part))
                && value.len() >= 8
            {
                self.secrets.push(value);
            }
        }
        for key in [
            "SSLKEYLOGFILE",
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "KIMI_SESSION_ID",
            "GEMINI_SESSION_ID",
            "OPENCODE_SESSION_ID",
        ] {
            environment.set(key, None);
        }
        for key in ["HOME", "USERPROFILE"] {
            environment.path(key, &self.home);
        }
        for (key, path) in [
            ("CODEX_HOME", self.home.join(".codex")),
            ("KIMI_CODE_HOME", self.home.join(".kimi-code")),
            ("GEMINI_CLI_HOME", self.home.clone()),
            ("OPENCODE_CONFIG_DIR", self.home.join("opencode-config")),
            ("XDG_CONFIG_HOME", self.home.join("xdg-config")),
            ("XDG_DATA_HOME", self.home.join("xdg-data")),
            ("XDG_CACHE_HOME", self.home.join("xdg-cache")),
            ("XDG_STATE_HOME", self.home.join("xdg-state")),
        ] {
            std::fs::create_dir_all(&path).map_err(|_| "probe/prepare_failed")?;
            environment.path(key, &path);
        }
        if self.provider == "gemini" {
            // 官方ACP telemetry会包含整段prompt与运行上下文，原件只能留在本次private-home；
            // 事后只保存固定字段的摘要，借此区分“prompt没到CLI”和“CLI没产生协议事件”。
            environment.set(
                "GEMINI_TELEMETRY_ENABLED",
                Some(std::ffi::OsStr::new("true")),
            );
            environment.set(
                "GEMINI_TELEMETRY_TARGET",
                Some(std::ffi::OsStr::new("local")),
            );
            environment.path(
                "GEMINI_TELEMETRY_OUTFILE",
                &self.home.join("gemini-telemetry.json"),
            );
        }
        environment.path("MEOWO_DB", &self.db);
        std::env::set_current_dir(&self.cwd).map_err(|_| "probe/prepare_failed")?;
        self.environment = Some(environment);
        let mut git = std::process::Command::new("git");
        git.args(["init", "--quiet"])
            .arg(&self.cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            git.creation_flags(0x0800_0000);
        }
        if !git.status().is_ok_and(|s| s.success()) {
            return Err("probe/git_unavailable".into());
        }
        let reporter = crate::setup::sibling_reporter().ok_or("probe/sibling_reporter_missing")?;
        let temporary = plugin.resolve().ok_or("probe/isolated_cli_missing")?;
        let canonical = dunce::canonicalize(&temporary.data_dir)
            .map_err(|_| "probe/isolated_config_missing")?;
        if !canonical
            .starts_with(dunce::canonicalize(&self.home).map_err(|_| "probe/isolation_failed")?)
        {
            return Err("probe/isolation_failed".into());
        }
        let cap = plugin.lead().ok_or("probe/lead_unsupported")?;
        let version = crate::probe_cli_version(plugin);
        let lead =
            meowo_agent::lead_install::install(cap, &temporary, version.as_deref(), &reporter)?;
        if !lead.installed {
            return Err("probe/lead_install_unverified".into());
        }
        let checkpoint = json!({"provider":self.provider,"stage":"auth_copied_no_model","nativeArgv":native_argv,"cwd":self.cwd,"privateHome":self.home,"db":self.db,"copiedFiles":self.copied,"sourceConfigModified":false});
        self.record("auth-checkpoint.json", &checkpoint)?;
        Ok(checkpoint)
    }
    fn redact(&self, value: &str) -> String {
        let mut out = value.to_owned();
        for secret in &self.secrets {
            if secret.len() >= 8 {
                out = out.replace(secret, "[REDACTED]");
            }
        }
        for pattern in [
            r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+",
            r#"(?i)((?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|authorization)\s*[=:]\s*[\"']?)[^\s,\"']+"#,
        ] {
            if let Ok(regex) = regex::Regex::new(pattern) {
                out = regex.replace_all(&out, "${1}[REDACTED]").into_owned();
            }
        }
        out
    }
    fn sanitized(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.redact(s).chars().take(8192).collect()),
            Value::Array(a) => Value::Array(a.iter().take(64).map(|v| self.sanitized(v)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| (k.clone(), self.sanitized(v)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    fn record(&self, name: &str, value: &Value) -> Result<(), String> {
        let raw = self.sanitized(value).to_string();
        if raw.len() > 128 * 1024 {
            return Err("probe/evidence_too_large".into());
        }
        std::fs::write(self.root.join(name), raw).map_err(|_| "probe/evidence_write_failed".into())
    }
    fn gemini_telemetry_diagnostic(&self, reason: &str) -> Result<(), String> {
        let path = self.home.join("gemini-telemetry.json");
        let bytes = std::fs::read(&path).ok();
        // 原始telemetry可能携带prompt或模型上下文，只允许在本次private-home中短暂存在。
        let _ = std::fs::remove_file(&path);
        let Some(bytes) = bytes else {
            return self.record(
                "gemini-telemetry-summary.json",
                &json!({"reason":reason,"available":false,"containsSecretValues":false}),
            );
        };
        let text = String::from_utf8_lossy(&bytes);
        let lower = text.to_ascii_lowercase();
        let non_empty = text.lines().filter(|line| !line.trim().is_empty()).count();
        let json_lines = text
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).is_ok())
            .count();
        self.record(
            "gemini-telemetry-summary.json",
            &json!({
                "reason":reason,
                "available":true,
                "bytes":bytes.len(),
                "nonEmptyLines":non_empty,
                "validJsonLines":json_lines,
                "hasSessionPrompt":text.contains("session/prompt"),
                "hasPromptSignal":lower.contains("prompt"),
                "hasMcpSignal":lower.contains("mcp"),
                "hasModelRequestSignal":lower.contains("generatecontent") || lower.contains("generativelanguage"),
                "hasAuthSignal":lower.contains("unauthorized") || lower.contains("authentication") || lower.contains("api key"),
                "hasRateLimitSignal":lower.contains("resource exhausted") || lower.contains("rate limit") || lower.contains("quota") || lower.contains("429"),
                "hasNetworkSignal":lower.contains("econn") || lower.contains("enotfound") || lower.contains("fetch failed"),
                "containsSecretValues":false
            }),
        )
    }
    fn diagnostic(&mut self, reason: &str) -> Result<(), String> {
        if self.provider == "gemini" {
            return self.gemini_telemetry_diagnostic(reason);
        }
        if self.provider == "opencode" {
            let id = self.id.ok_or("probe/session_missing")?;
            let mut diagnostics = vec![];
            for event in super::events(&self.db, id, 0, 256)?.events {
                let payload = event.payload;
                if payload["kind"] != "native_diagnostic" || diagnostics.len() >= 8 {
                    continue;
                }
                let allowed = |key: &str, values: &[&str]| {
                    payload[key]
                        .as_str()
                        .filter(|value| values.contains(value))
                        .map(str::to_owned)
                };
                diagnostics.push(json!({
                    "transport": allowed("transport", &["sse"]),
                    "signal": allowed("signal", &["message.updated.info.error", "session.error"]),
                    "category": allowed("category", &["auth", "rate_limited", "timeout", "network", "context_limit", "model_or_provider", "unknown"]),
                    "errorShape": allowed("error_shape", &["missing", "string", "object", "array", "scalar"]),
                    "status": payload["status"].as_u64().filter(|status| (100..=599).contains(status)),
                }));
            }
            // 只读取本次私有DB中的白名单枚举；原生错误文本、请求体和认证配置均不落盘。
            return self.record("diagnostic.json", &json!({"provider":"opencode","reason":reason,"scope":"private managed event classifications only","nativeDiagnostics":diagnostics,"sourceConfigDumped":false}));
        }
        if self.provider != "kimi" {
            return Ok(());
        }
        // 刷新后的OAuth值也先进入脱敏表；只读私有副本，不回读/写源账号配置。
        for relative in self.copied.clone() {
            let path = self.home.join(relative);
            if path.extension().is_some_and(|s| s == "json") {
                if let Ok(Some(value)) = source_json(&path) {
                    strings(&value, &mut self.secrets);
                }
            } else if path.extension().is_some_and(|s| s == "toml") {
                if let Ok(Some(doc)) = source_toml(&path) {
                    toml_secrets(doc.as_item(), &mut self.secrets);
                }
            }
        }
        let mut pending = vec![(self.home.join(".kimi-code"), 0)];
        let mut files = vec![];
        let private_home = dunce::canonicalize(&self.home).map_err(|_| "probe/diagnostic_scope")?;
        while let Some((dir, depth)) = pending.pop() {
            if depth > 4 || files.len() >= 16 {
                continue;
            }
            if !dunce::canonicalize(&dir).is_ok_and(|p| p.starts_with(&private_home)) {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten().take(256) {
                let path = entry.path();
                if !dunce::canonicalize(&path).is_ok_and(|p| p.starts_with(&private_home)) {
                    continue;
                }
                if path.is_dir() {
                    if ![
                        "credentials",
                        "sessions",
                        "skills",
                        "agents",
                        "plugins",
                        "plugin",
                    ]
                    .contains(&entry.file_name().to_string_lossy().as_ref())
                    {
                        pending.push((path, depth + 1));
                    }
                } else if path.extension().is_some_and(|s| s == "log") {
                    files.push(path);
                    if files.len() >= 16 {
                        break;
                    }
                }
            }
        }
        let mut lines = vec![];
        for path in files {
            use std::io::{Seek, SeekFrom};
            let Ok(mut file) = std::fs::File::open(path) else {
                continue;
            };
            let Ok(size) = file.metadata().map(|m| m.len()) else {
                continue;
            };
            let _ = file.seek(SeekFrom::Start(size.saturating_sub(128 * 1024)));
            let mut raw = String::new();
            let _ = file.take(128 * 1024).read_to_string(&mut raw);
            for line in raw
                .lines()
                .rev()
                .filter(|line| {
                    let line = line.to_ascii_lowercase();
                    [
                        "auth",
                        "credential",
                        "unauthorized",
                        "401",
                        "403",
                        "ready",
                        "provider",
                        "network",
                        "timeout",
                    ]
                    .iter()
                    .any(|key| line.contains(key))
                })
                .take(12)
            {
                lines.push(self.redact(line).chars().take(1200).collect::<String>());
                if lines.len() >= 24 {
                    break;
                }
            }
            if lines.len() >= 24 {
                break;
            }
        }
        self.record("diagnostic.json",&json!({"provider":"kimi","reason":reason,"scope":"private log tails, readiness/auth lines only","lines":lines,"sourceConfigDumped":false}))
    }
    fn shutdown(&self) -> bool {
        let Some(id) = self.id else { return true };
        let _ = super::control(&self.db, id, "stop", None);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let state = Store::open(&self.db)
                .ok()
                .and_then(|s| s.managed_state(id).ok().flatten());
            if state.is_some_and(|s| s.runtime_owner.is_none()) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn cleanup(&mut self) -> bool {
        let stopped = self.shutdown();
        if self
            .home
            .canonicalize()
            .ok()
            .is_some_and(|p| p.parent() == self.root.canonicalize().ok().as_deref())
        {
            let _ = std::fs::remove_dir_all(&self.home);
        }
        stopped && !self.home.exists()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    id: String,
    choice: Option<String>,
    #[serde(default)]
    answers: Value,
}

fn approval(sandbox: &Sandbox, stage: &str) -> Result<bool, String> {
    let id = sandbox.id.ok_or("probe/session_missing")?;
    let Some(interaction) = super::interaction(&sandbox.db, id)? else {
        return Ok(false);
    };
    sandbox.record("probe-state.json",&json!({"provider":sandbox.provider,"stage":stage,"sessionId":id,"cwd":sandbox.cwd,"interaction":interaction,"awaiting":"approval.json","automaticAllow":false}))?;
    let Event::Interaction { interaction_id, .. } = &interaction else {
        return Ok(false);
    };
    let deadline = Instant::now() + APPROVAL_WAIT;
    loop {
        if !super::interaction(&sandbox.db,id)?.is_some_and(|event|matches!(event,Event::Interaction {interaction_id:current,..}if current==*interaction_id)){return Ok(true);}
        if let Some(raw) = small(&sandbox.root.join("approval.json"))? {
            let answer: Approval =
                serde_json::from_slice(&raw).map_err(|_| "probe/approval_invalid")?;
            if answer.id != *interaction_id {
                return Err("probe/approval_wrong_request".into());
            }
            super::resolve(
                &sandbox.db,
                id,
                &answer.id,
                answer.choice.as_deref(),
                answer.answers,
            )?;
            std::fs::remove_file(sandbox.root.join("approval.json"))
                .map_err(|_| "probe/approval_remove_failed")?;
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Err("probe/approval_timeout".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn ready(sandbox: &Sandbox) -> Result<ManagedConversationDto, String> {
    let mut deadline = Instant::now() + Duration::from_secs(60);
    let id = sandbox.id.ok_or("probe/session_missing")?;
    loop {
        let state = super::state(&sandbox.db, id)?.ok_or("probe/session_missing")?;
        if state.phase == "ready" {
            return Ok(state);
        }
        if state.phase == "failed" {
            return Err(state
                .last_error
                .unwrap_or_else(|| "probe/native_failed_unverified".into()));
        }
        if Instant::now() >= deadline {
            return Err("probe/ready_timeout".into());
        }
        if approval(sandbox, "native-ready")? {
            deadline = Instant::now() + Duration::from_secs(60);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn output_nonce(sandbox: &Sandbox, nonce: &str, stage: &str) -> Result<Value, String> {
    let id = sandbox.id.ok_or("probe/session_missing")?;
    let mut deadline = Instant::now() + Duration::from_secs(120);
    let mut sequence = 0;
    let mut output = String::new();
    loop {
        let page = super::events(&sandbox.db, id, sequence, 256)?;
        for event in page.events {
            sequence = event.sequence;
            if event.payload["kind"] == "output" && event.payload["reasoning"] != true {
                if let Some(text) = event.payload["text"].as_str() {
                    if output.len() + text.len() > 64 * 1024 {
                        return Err("probe/output_limit".into());
                    }
                    output.push_str(text);
                }
            }
        }
        let state = super::state(&sandbox.db, id)?.ok_or("probe/session_missing")?;
        if state.phase == "ready" && output.contains(nonce) {
            return Ok(
                json!({"actualOutputContainsNonce":true,"nonce":nonce,"nativeSessionId":state.native_session_id,"deliveries":super::deliveries(&sandbox.db,id)?}),
            );
        }
        if state.phase == "failed" {
            return Err(state
                .last_error
                .unwrap_or_else(|| "probe/native_failed_unverified".into()));
        }
        if approval(sandbox, stage)? {
            deadline = Instant::now() + Duration::from_secs(120);
            continue;
        }
        if Instant::now() >= deadline {
            return Err("probe/output_timeout".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn gate(sandbox: &Sandbox, native: &str, gate_id: &str) -> Result<PathBuf, String> {
    let task = sandbox.cwd.join(".lead/managed-probe");
    std::fs::create_dir_all(&task).map_err(|_| "probe/task_write_failed")?;
    for (name,body) in [(".owner",native.to_owned()),("SPEC.md","Local isolated managed Lead continuation probe. Only touch this task directory. Keep phase blocked while the probe controls the next step; consume answered gates and echo the nonce, without starting unrelated work.".into()),("progress.md","phase: blocked\n".into()),("acceptance.json","[]".into()),("gate.json",json!({"id":gate_id,"type":"Q","status":"pending","title":"Managed continuation probe","summary":"Reply with the exact nonce in the leader note. Consume the gate using normal Lead rules; do not change authentication or other files.","options":[{"id":"go","label":"Continue"}]}).to_string())]{std::fs::write(task.join(name),body).map_err(|_|"probe/task_write_failed")?;}
    Ok(task)
}

fn run(sandbox: &mut Sandbox, models: bool) -> Result<Value, String> {
    let state = super::create(
        &sandbox.db,
        &sandbox.provider,
        &sandbox.cwd.to_string_lossy(),
        None,
        None,
    )?;
    sandbox.id = Some(state.session_id);
    let state = ready(sandbox)?;
    let native = state
        .native_session_id
        .clone()
        .ok_or("probe/native_identity_missing")?;
    sandbox.record("probe-state.json",&json!({"stage":"ready_no_model","provider":sandbox.provider,"sessionId":state.session_id,"nativeSessionId":native,"cwd":sandbox.cwd}))?;
    if !models {
        return Ok(
            json!({"status":"handshake_only","nativeSessionId":native,"modelInvoked":false}),
        );
    }
    let nonce = format!("WARM_{}", crate::remote::generate_token()?);
    super::send(&sandbox.db,state.session_id,&format!("warm-{nonce}"),&format!("This is an isolated local Lead continuation test. This first turn: reply exactly {nonce}, no tools. Later leader answers may arrive for .lead/managed-probe/gate.json. On each answer, follow the normal Lead consume rules, touch only that task directory, and end by replying with the exact nonce in the leader note. Never change authentication/configuration or run unrelated commands."),&[],None,None)?;
    let warm = output_nonce(sandbox, &nonce, "warm")?;
    let gate1 = format!("online-{}", crate::remote::generate_token()?);
    let task = gate(sandbox, &native, &gate1)?;
    let online_nonce = format!("ONLINE_{}", crate::remote::generate_token()?);
    let answer=LeadGateAnswerDto {choice:"go".into(),note:format!("Reply exactly {online_nonce} after following the normal gate consume rules. Touch only this isolated task directory.")};
    if !crate::lead_conversation::answer_and_deliver(
        &sandbox.db,
        &sandbox.cwd.to_string_lossy(),
        &gate1,
        &answer,
        None,
    )? {
        return Err("probe/legacy_route_unexpected".into());
    }
    let online = output_nonce(sandbox, &online_nonce, "online-gate")?;
    let online_consumed =
        meowo_reporter::lead::read_gate(&task).is_some_and(|g| g.status == "consumed");
    if !sandbox.shutdown() {
        return Err("probe/exit_unverified".into());
    }
    let gate2 = format!("offline-{}", crate::remote::generate_token()?);
    gate(sandbox, &native, &gate2)?;
    let offline_nonce = format!("OFFLINE_{}", crate::remote::generate_token()?);
    let answer=LeadGateAnswerDto {choice:"go".into(),note:format!("Reply exactly {offline_nonce} after following the normal gate consume rules. Touch only this isolated task directory.")};
    if !crate::lead_conversation::answer_and_deliver(
        &sandbox.db,
        &sandbox.cwd.to_string_lossy(),
        &gate2,
        &answer,
        None,
    )? {
        return Err("probe/legacy_route_unexpected".into());
    }
    let offline = output_nonce(sandbox, &offline_nonce, "offline-gate")?;
    let restored = super::state(&sandbox.db, state.session_id)?.ok_or("probe/session_missing")?;
    if restored.native_session_id.as_deref() != Some(&native) {
        return Err("probe/native_root_changed".into());
    }
    let owner = std::fs::read_to_string(task.join(".owner")).map_err(|_| "probe/owner_missing")?;
    if owner.trim() != native {
        return Err("probe/owner_changed".into());
    }
    let offline_consumed =
        meowo_reporter::lead::read_gate(&task).is_some_and(|g| g.status == "consumed");
    let claim_nonce = format!("CLAIM_{}", crate::remote::generate_token()?);
    super::send(&sandbox.db,state.session_id,&format!("claim-{claim_nonce}"),&format!("Use your native file-write tool to create .lead/native-write-probe/SPEC.md containing only this isolated test description and {claim_nonce}. Do not delegate. Do not create or edit .owner, active, authentication, configuration, or any other path. Then reply exactly {claim_nonce}."),&[],None,None)?;
    let claim = output_nonce(sandbox, &claim_nonce, "native-tool-claim")?;
    let claimed_owner =
        std::fs::read_to_string(sandbox.cwd.join(".lead/native-write-probe/.owner"))
            .map_err(|_| "probe/native_claim_missing")?;
    if claimed_owner.trim() != native {
        return Err("probe/native_claim_wrong_root".into());
    }
    Ok(
        json!({"status":"model_rounds_observed","warm":warm,"online":online,"offline":offline,"nativeToolClaim":claim,"sameNativeRoot":true,"nativeClaimOwnerMatchesRoot":true,"onlineBusinessGateConsumed":online_consumed,"offlineBusinessGateConsumed":offline_consumed}),
    )
}

/// 用同一私有认证环境验证模型服务本身；输出只在内存里比对随机nonce，原文立即删除。
fn gemini_native_ping(sandbox: &Sandbox) -> Result<Value, String> {
    if sandbox.provider != "gemini" {
        return Err("probe/provider_invalid".into());
    }
    let plugin = meowo_agent::by_id("gemini").ok_or("probe/provider_invalid")?;
    let installation = plugin.resolve().ok_or("probe/isolated_cli_missing")?;
    let mut argv = installation
        .launch
        .clone()
        .unwrap_or_else(|| plugin.launch_argv());
    let command = argv.first().cloned().ok_or("probe/isolated_cli_missing")?;
    argv.remove(0);
    let nonce = format!("PING_{}", crate::remote::generate_token()?);
    let stdout_path = sandbox.home.join("gemini-native.stdout");
    let stderr_path = sandbox.home.join("gemini-native.stderr");
    let stdout = std::fs::File::create(&stdout_path).map_err(|_| "probe/output_open_failed")?;
    let stderr = std::fs::File::create(&stderr_path).map_err(|_| "probe/output_open_failed")?;
    let prompt = format!("Reply with exactly {nonce}");
    let mut command = std::process::Command::new(command);
    let model = std::env::var("MEOWO_PROBE_GEMINI_MODEL")
        .ok()
        .filter(|model| {
            !model.is_empty()
                && model
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        });
    command
        .args(argv)
        .args(["--prompt", prompt.as_str(), "--yolo"]);
    if let Some(model) = model.as_deref() {
        // 模型名只来自受限探针变量；显式选低负载模型，避免默认模型503的退避掩盖ACP闭环。
        command.args(["--model", model]);
    }
    command
        .current_dir(&sandbox.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| "probe/native_launch_failed")?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        match child.try_wait().map_err(|_| "probe/native_wait_failed")? {
            Some(status) => break Some(status),
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = small(&stdout_path)?.unwrap_or_default();
    let stderr = small(&stderr_path)?.unwrap_or_default();
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);
    let stderr = String::from_utf8_lossy(&stderr).to_ascii_lowercase();
    let output_contains_nonce = String::from_utf8_lossy(&stdout).contains(&nonce);
    let summary = json!({
        "statusCode":status.and_then(|value| value.code()),
        "timedOut":status.is_none(),
        "outputContainsNonce":output_contains_nonce,
        "authSignal":stderr.contains("unauthorized") || stderr.contains("authentication") || stderr.contains("api key"),
        "rateLimitSignal":stderr.contains("resource exhausted") || stderr.contains("rate limit") || stderr.contains("quota") || stderr.contains("429"),
        "networkSignal":stderr.contains("econn") || stderr.contains("enotfound") || stderr.contains("fetch failed"),
        // 原始stderr可能包含本机路径或认证上下文；保留互斥能力无关的分类位，才可把CLI本身故障
        // 与ACP传输故障拆开，同时不把原文带入长期证据。
        "configSignal":stderr.contains("settings") || stderr.contains("config") || stderr.contains("validation"),
        "workspaceTrustSignal":stderr.contains("trust") && (stderr.contains("folder") || stderr.contains("workspace") || stderr.contains("directory")),
        "consentSignal":stderr.contains("consent") || stderr.contains("terms"),
        "inputSignal":stderr.contains("stdin") || stderr.contains("non-interactive") || stderr.contains("input required"),
        "containsSecretValues":false
    });
    sandbox.record("gemini-native-ping.json", &summary)?;
    if output_contains_nonce && status.is_some_and(|value| value.success()) {
        Ok(summary)
    } else {
        Err("probe/native_model_unverified".into())
    }
}

#[test]
#[ignore = "独立进程：显式MEOWO_PROBE_PROVIDER+MODE；models模式另需Root授权开关，默认不运行模型"]
fn manager_provider_model_closure() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.iter().any(|a| a == "--exact") && args.iter().any(|a| a == "--test-threads=1"),
        "probe requires one exact test in a dedicated process"
    );
    let provider = std::env::var("MEOWO_PROBE_PROVIDER").expect("probe provider required");
    assert!(["codex", "kimi", "gemini", "opencode"].contains(&provider.as_str()));
    let mode = std::env::var("MEOWO_PROBE_MODE").expect("handshake or models required");
    assert!(["handshake", "models"].contains(&mode.as_str()));
    let models = mode == "models";
    if models {
        assert_eq!(
            std::env::var("MEOWO_RUN_MANAGER_MODELS").as_deref(),
            Ok("root-authorized"),
            "models remain disabled until Root explicitly authorizes"
        );
    }
    let mut sandbox = Sandbox::new(&provider).expect("probe sandbox creation failed");
    let prepared = sandbox.prepare();
    let result = prepared.and_then(|_| run(&mut sandbox, models));
    if let Err(reason) = &result {
        let _ = sandbox.diagnostic(reason);
    }
    let cleanup = sandbox.cleanup();
    let evidence = match &result {
        Ok(value) => json!({"result":value,"cleanupVerified":cleanup}),
        Err(reason) => {
            json!({"status":"unverified","reason":reason,"cause":"environment_or_native_failure_requires_evidence","cleanupVerified":cleanup})
        }
    };
    sandbox
        .record("result.json", &evidence)
        .expect("probe evidence failed");
    eprintln!(
        "managed probe evidence: {}",
        sandbox.root.join("result.json").display()
    );
    assert!(
        cleanup,
        "probe cleanup unverified; inspect evidence before any retry"
    );
    assert!(
        result.is_ok(),
        "probe unverified; inspect structured evidence, do not blindly retry"
    );
}

#[test]
#[ignore = "独立进程：使用private-home验证Gemini非ACP模型请求，默认不运行模型"]
fn gemini_native_model_ping_probe() {
    assert_eq!(
        std::env::var("MEOWO_RUN_MANAGER_MODELS").as_deref(),
        Ok("root-authorized"),
        "models remain disabled until Root explicitly authorizes"
    );
    let mut sandbox = Sandbox::new("gemini").expect("probe sandbox creation failed");
    let result = sandbox.prepare().and_then(|_| gemini_native_ping(&sandbox));
    let cleanup = sandbox.cleanup();
    let evidence = match &result {
        Ok(value) => json!({"result":value,"cleanupVerified":cleanup}),
        Err(reason) => {
            json!({"status":"unverified","reason":reason,"cause":"environment_or_native_failure_requires_evidence","cleanupVerified":cleanup})
        }
    };
    sandbox
        .record("native-ping-result.json", &evidence)
        .expect("probe evidence failed");
    eprintln!(
        "native Gemini ping evidence: {}",
        sandbox.root.join("native-ping-result.json").display()
    );
    assert!(
        cleanup,
        "probe cleanup unverified; inspect evidence before any retry"
    );
    assert!(
        result.is_ok(),
        "native model unverified; inspect structured evidence, do not blindly retry"
    );
}

#[test]
fn gemini_telemetry_diagnostic_records_only_counts_and_removes_raw_file() {
    let mut sandbox = Sandbox::new("gemini").unwrap();
    let raw = sandbox.home.join("gemini-telemetry.json");
    std::fs::write(
        &raw,
        "{\"method\":\"session/prompt\",\"authorization\":\"private-auth-example\"}\n",
    )
    .unwrap();
    sandbox.secrets.push("private-auth-example".into());
    sandbox.diagnostic("probe/output_timeout").unwrap();
    assert!(!raw.exists());
    let summary =
        std::fs::read_to_string(sandbox.root.join("gemini-telemetry-summary.json")).unwrap();
    assert!(summary.contains("hasSessionPrompt") && !summary.contains("private-auth-example"));
    let _ = std::fs::remove_dir_all(&sandbox.root);
}

#[test]
fn secret_redaction_never_uses_text_as_a_delivery_identity() {
    let mut sandbox = Sandbox::new("codex").unwrap();
    sandbox.secrets.push("private-auth-example".into());
    assert_eq!(
        sandbox.redact("keep nonce, hide private-auth-example"),
        "keep nonce, hide [REDACTED]"
    );
    let _ = std::fs::remove_dir_all(&sandbox.root);
}

#[test]
fn kimi_minimal_clone_roundtrip_preserves_selected_model_and_provider() {
    let source: toml_edit::DocumentMut = r#"default_model = "custom/model"
default_permission_mode = "manual"
[[hooks]]
event = "SessionStart"
[[hooks.hooks]]
command = "fixture-unrelated-hook"
[thinking]
enabled = true
effort = "high"
[services.moonshot_search]
base_url = "https://search.invalid.example"
api_key = "unused-service-fixture"
[services.moonshot_fetch]
base_url = "https://fetch.invalid.example"
api_key = "unused-service-fixture"
[providers."unused-first"]
base_url = "https://unused.invalid.example"
type = "openai"
api_key = "unused-provider-fixture"
[providers."自定义"]
base_url = "https://invalid.example"
type = "anthropic"
api_key = "fixture-value"
custom_headers = { "X-Fixture-Header" = "header-value" }
env = { MODEL_FIXTURE_OPTION = "option-value" }
[models."unused-first/model"]
provider = "unused-first"
model = "unused-model"
max_context_size = 2000
[models."custom/model"]
provider = "自定义"
model = "fixture-model"
max_context_size = 1000
capabilities = ["thinking", "image"]
support_efforts = ["low", "high"]
default_effort = "high"
[experimental]
notify_user = true
"#
    .parse()
    .unwrap();
    let out = kimi_clone_config(&source);
    let reparsed: toml_edit::DocumentMut = out.to_string().parse().unwrap();
    assert_eq!(kimi_config_shape(&out), kimi_config_shape(&reparsed));
    assert_eq!(
        kimi_config_shape(&reparsed)["referencedProviderPresent"],
        true
    );
    assert_eq!(
        kimi_config_shape(&reparsed)["providerInlineKeyPresent"],
        true
    );
    assert_eq!(
        reparsed["models"]["custom/model"]["capabilities"][1].as_str(),
        Some("image")
    );
    assert_eq!(
        reparsed["providers"]["自定义"]["custom_headers"]["X-Fixture-Header"].as_str(),
        Some("header-value")
    );
    assert_eq!(
        reparsed["providers"]["自定义"]["env"]["MODEL_FIXTURE_OPTION"].as_str(),
        Some("option-value")
    );
    assert!(reparsed.get("hooks").is_none());
    assert!(reparsed.get("thinking").is_none());
    assert!(reparsed.get("services").is_none());
    assert!(reparsed["providers"].get("unused-first").is_none());
    assert!(reparsed["models"].get("unused-first/model").is_none());
}
