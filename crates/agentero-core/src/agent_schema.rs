//! Shared Agent registry serialization used by Host and CLI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AgentTemplate {
    Opencode,
    /// Hermes Agent native ACP (`hermes acp`).
    /// Docs: https://github.com/NousResearch/hermes-agent
    Hermes,
    ClaudeAcp,
    CodexAcp,
    /// Cursor Agent CLI with native ACP (`cursor-agent acp`).
    /// Docs: https://cursor.com/docs/cli
    CursorAgent,
    /// Google Antigravity CLI with native ACP (`agy acp`).
    AntigravityAcp,
    /// Qoder CLI native ACP (`qodercli --acp`).
    /// Docs: https://docs.qoder.com/en/cli/acp
    QoderCli,
    /// Qoder CN CLI (mainland build) with native ACP (`qodercn --acp`).
    /// Docs: https://docs.qoder.cn/cli/acp
    QoderCn,
    /// Grok Build native ACP (`grok agent stdio`).
    /// Docs: https://zed.dev/acp/agent/grok-build
    GrokBuild,
    /// Pi coding agent via the community `pi-acp` adapter (pi has no native ACP).
    /// Docs: https://pi.dev · https://github.com/svkozak/pi-acp
    Pi,
    /// DeepSeek Harness umbrella CLI with native ACP (`dsh --profile acp`).
    /// Docs: https://github.com/deepseek-ai/deepseek-harness
    Dsh,
    /// Moonshot Kimi Code CLI with native ACP (`kimi acp`).
    /// Docs: https://moonshotai.github.io/kimi-code/en/
    KimiCode,
    /// ZCode CLI via the community `zcode-acp-server` adapter, which bridges the
    /// headless `zcode app-server --stdio`. Reuses the ZCode desktop app login
    /// (`~/.zcode`); the adapter auto-discovers the app-bundled CLI.
    /// Docs: https://github.com/william0wang/zcode-acp
    Zcode,
    /// MiniMax Code CLI with native ACP (`mcode acp`).
    /// Docs: https://agent.minimax.io/docs/cli/quick-start
    MinimaxCode,
    /// Xiaomi MiMo Code CLI (an OpenCode fork) with native ACP (`mimo acp`).
    /// Docs: https://mimo.xiaomi.com · npm package `mimocode`
    MimoCode,
    /// Tencent CodeBuddy Code CLI with native ACP (`codebuddy --acp`).
    /// Docs: https://www.codebuddy.ai/docs/cli/acp · npm package `@tencent-ai/codebuddy-code`
    Codebuddy,
    /// 内置「知乎」Agent：Host 内建，不 spawn 外部进程，直连知乎开放平台
    /// （直答 + 标题检索）。凭证来自 App settings。
    Zhihu,
    Custom,
}

/// 内建 Agent（Host 内建适配器，无外部 ACP 进程）。前端聊天气泡、Settings
/// 卡片等按此区分：这些 Agent 不经 ACP spawn / probe / session list。
pub fn is_builtin_template(template: &AgentTemplate) -> bool {
    matches!(template, AgentTemplate::Zhihu)
}

/// 内置「知乎」Agent 的稳定 registry id。
pub const ZHIHU_AGENT_ID: &str = "builtin-zhihu";

impl<'de> serde::Deserialize<'de> for AgentTemplate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "opencode" => Self::Opencode,
            // Legacy OpenClaw preset (removed): keep old registries loading as
            // a custom agent instead of failing the whole registry parse.
            "openclaw" => Self::Custom,
            "hermes" => Self::Hermes,
            "claude-acp" => Self::ClaudeAcp,
            "codex-acp" => Self::CodexAcp,
            "cursor-agent" => Self::CursorAgent,
            "antigravity-acp" => Self::AntigravityAcp,
            "qodercli" => Self::QoderCli,
            "qoder-cn" => Self::QoderCn,
            "grok-build" => Self::GrokBuild,
            "pi" => Self::Pi,
            "dsh" => Self::Dsh,
            "kimi-code" => Self::KimiCode,
            "zcode" => Self::Zcode,
            "minimax-code" => Self::MinimaxCode,
            "mimo-code" => Self::MimoCode,
            "codebuddy" => Self::Codebuddy,
            "zhihu" => Self::Zhihu,
            "custom" => Self::Custom,
            other => {
                return Err(serde::de::Error::custom(format!(
                    "unknown agent template: {other}"
                )))
            }
        })
    }
}

impl AgentTemplate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Hermes => "hermes",
            Self::ClaudeAcp => "claude-acp",
            Self::CodexAcp => "codex-acp",
            Self::CursorAgent => "cursor-agent",
            Self::AntigravityAcp => "antigravity-acp",
            Self::QoderCli => "qodercli",
            Self::QoderCn => "qoder-cn",
            Self::GrokBuild => "grok-build",
            Self::Pi => "pi",
            Self::Dsh => "dsh",
            Self::KimiCode => "kimi-code",
            Self::Zcode => "zcode",
            Self::MinimaxCode => "minimax-code",
            Self::MimoCode => "mimo-code",
            Self::Codebuddy => "codebuddy",
            Self::Zhihu => "zhihu",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
    pub template: AgentTemplate,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Last ACP probe succeeded (None = never probed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probe_ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probe_agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probe_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_probed_at: Option<String>,
}

/// Anonymous, telemetry-safe summary of registered agents: kebab-case
/// template ids for catalog agents and a bare count for custom ones. Never
/// carries agent names, commands, args, or env.
#[derive(Debug, Clone, Default)]
pub struct AgentTelemetrySummary {
    pub templates: Vec<String>,
    pub custom_count: usize,
}

pub const DEFAULT_AGENT_PROXY_URL: &str = "http://127.0.0.1:7890";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRegistryState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_id: Option<String>,
    #[serde(default)]
    pub agents: Vec<AgentDescriptor>,
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default = "default_agent_proxy_url")]
    pub proxy_url: String,
    /// Optional HTTP User-Agent injected into ACP agent processes (esp. Codex).
    /// Empty = do not override. Relays that only allow Codex often expect
    /// `codex-cli/<version>` (see new-api channel affinity).
    #[serde(default)]
    pub user_agent: String,
    /// Comma-separated Codex `model_providers` ids that receive `http_headers.User-Agent`.
    /// Empty = auto (existing CODEX_CONFIG providers + MODEL_PROVIDER + `openai`).
    #[serde(default)]
    pub user_agent_provider_ids: String,
}

impl Default for AgentRegistryState {
    fn default() -> Self {
        Self {
            enabled: true,
            default_id: None,
            agents: Vec::new(),
            proxy_enabled: false,
            proxy_url: default_agent_proxy_url(),
            user_agent: String::new(),
            user_agent_provider_ids: String::new(),
        }
    }
}

pub fn default_agent_proxy_url() -> String {
    DEFAULT_AGENT_PROXY_URL.to_string()
}
