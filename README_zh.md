# Astro-Member: 多层级记忆体 MCP 服务端

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![MCP](https://img.shields.io/badge/protocol-Model%20Context%20Protocol-blue.svg)](https://modelcontextprotocol.io/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](#license)

`astro-member` 是一个基于 Rust 编写的高性能、轻量级 **Model Context Protocol (MCP) 服务端**。它为大语言模型（LLM）智能体提供了一个外置的长期记忆系统，具备多层级记忆体架构、图语义关联、时间指数衰减以及双轨（Dense 向量匹配 + Sparse 关键词检索）混合搜索特性。

项目摒弃了繁重的外置向量数据库，采用嵌入式 SQLite 数据库及本地嵌入模型（`fastembed`），实现了无缝自包含、低时延的语义索引与关联存储。

---

## 📚 文档导航

为了确保文档清晰和结构隔离，项目将 README 与详细的技术文档和 API 手册进行了区分，并分别提供中英文版本：

*   **英文文档 / English Documents**:
    *   [README.md](file:///d:/Project/AiProject/astro-member/README.md) - 项目概述与快速上手（英文主页）。
    *   [TECHNICAL_DOC.md](file:///d:/Project/AiProject/astro-member/TECHNICAL_DOC.md) - 详细的系统架构、MCP 工具 API 接口规范、参数限制、JSON-RPC 载荷示例及环境变量配置。
*   **中文文档**:
    *   [README_zh.md](file:///d:/Project/AiProject/astro-member/README_zh.md) - 本文档，提供中文项目概述与快速上手说明。
    *   [TECHNICAL_DOC_zh.md](file:///d:/Project/AiProject/astro-member/TECHNICAL_DOC_zh.md) - 包含系统架构设计、多层级记忆细节、工具接口详细规范（包含可选参数说明与完整 JSON-RPC 2.0 示例）以及环境配置规范。

---

## ✨ 核心特性亮点

*   **多层级记忆体架构**：记忆划分为四个逻辑层级（规则层、人设层、经验层、会话层），各自拥有不同的基础权重及时间指数衰减速度。
*   **图语义关联关系**：使用有向且带类型的语义关联（如 `depends_on`, `related_to`）将记忆片段连接，且内置引用完整性和级联删除。
*   **双轨混合搜索引擎**：结合了 Dense（通过 FastEmbed 自动计算向量并使用余弦相似度匹配）与 Sparse（基于 BM25 词频算法的内存级硬匹配）检索。
*   **经验强化机制**：根据大模型的成功/失败运行反馈，动态乘算强化系数（`0.1` 至 `5.0`），使智能体学会选择有效的问题解决方案。

---

## 🚀 快速上手

### 准备工作

*   **Rust**：稳定的工具链 (Edition 2021)
*   **Windows 开发编译环境**：请使用 **Developer Command Prompt for VS 2022** 进行编译，以确保 MSVC 构建链能正确加载。

### 编译运行

使用标准 Cargo 编译 Release 二进制文件：
```bash
cargo build --release
```
编译后的可执行文件将位于 `target/release/astro-member.exe`（Windows 环境）或 `target/release/astro-member`（macOS/Linux 环境）。

### 集成到 Claude 客户端

通过修改您的 `claude_desktop_config.json` 配置文件，将 `astro-member` 注册为 MCP 服务端：
*   **Windows (PowerShell)**: `%APPDATA%\Claude\claude_desktop_config.json`
*   **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`

配置文件示例（请将可执行文件路径修改为您本地的绝对路径）：
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

重新启动 Claude Desktop，您将在输入框区域看到 `astro-member` 工具的拼图图标。

---

## 📄 开源协议

本项目遵循 MIT 开源许可协议，详情请参阅 [LICENSE](LICENSE) 文件。
