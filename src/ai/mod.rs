//! AI integration: the editor tools (`commands`), the local socket the app
//! serves them on (`server`), the stdio MCP bridge for Codex and Claude Code
//! (`mcp`), and the in-app chat against the Claude and OpenAI APIs (`chat`).

pub mod chat;
pub mod commands;
pub mod mcp;
pub mod server;
