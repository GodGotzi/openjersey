//! AI integration: the editor tools (`commands`), the local socket the app
//! serves them on (`server`), the stdio MCP bridge for Codex and Claude Code
//! (`mcp`).

pub mod commands;
pub mod log;
pub mod mcp;
pub mod server;
