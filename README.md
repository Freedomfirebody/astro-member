# Astro-Member: Hierarchical Memory MCP Server

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![MCP](https://img.shields.io/badge/protocol-Model%20Context%20Protocol-blue.svg)](https://modelcontextprotocol.io/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](#license)

`astro-member` is a lightweight, high-performance **Model Context Protocol (MCP) Server** written in Rust. It serves as an external memory system for LLM agents, featuring hierarchical memory layers, graph semantic associations, temporal decay, and hybrid dense/sparse search.

---

## 📚 Document Navigation / 文档导航

To keep the documentation clean and separated, we provide distinct files for the general README and the detailed Technical/API Reference in both English and Chinese:

*   **English Documents**:
    *   [README.md](file:///d:/Project/AiProject/astro-member/README.md) - General overview and quickstart (this file).
    *   [TECHNICAL_DOC.md](file:///d:/Project/AiProject/astro-member/TECHNICAL_DOC.md) - Detailed architecture, tool API specifications, parameter constraints, JSON-RPC examples, and environment configuration.
*   **Chinese Documents / 中文文档**:
    *   [README_zh.md](file:///d:/Project/AiProject/astro-member/README_zh.md) - 项目简介与快速上手说明。
    *   [TECHNICAL_DOC_zh.md](file:///d:/Project/AiProject/astro-member/TECHNICAL_DOC_zh.md) - 详细的系统架构设计、MCP 工具接口参数规范、JSON-RPC 示例及环境配置指南。

---

## ✨ Features Highlight

*   **Hierarchical Memory Layers**: Four logical layers (Rule, Persona, Experience, Session) with different base weights and temporal decay characteristics.
*   **Semantic Graph Associations**: Connect memories with typed relations (e.g. `depends_on`, `related_to`) with referential integrity.
*   **Dual-Track Hybrid Search**: Combines Dense (FastEmbed vector embedding matching) and Sparse (BM25 exact keyword matching) retrieval.
*   **Experience Reinforcement**: Dynamically boosts or penalizes problem-solving experience memory scores based on success/failure feedback.

---

## 🚀 Quick Start

### Prerequisites

*   **Rust**: Stable toolchain (Edition 2021)
*   **Developer Command Prompt for VS 2022** (on Windows) to compile with the required MSVC toolchain.

### Compilation

Build the release binary:
```bash
cargo build --release
```
The compiled executable will be located at `target/release/astro-member.exe` (on Windows) or `target/release/astro-member` (on macOS/Linux).

### Claude Desktop Integration

Add `astro-member` as an MCP server by modifying your `claude_desktop_config.json` configuration file:
*   **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`
*   **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`

Configuration:
```json
{
  "mcpServers": {
    "astro-member": {
      "command": "d:\\Project\\AiProject\\astro-member\\target\\release\\astro-member.exe",
      "args": []
    }
  }
}
```

Restart Claude Desktop, and you will see the `astro-member` tool icon appear in the chat input area.

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
