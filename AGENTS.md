# Repository Guidelines

MoviePilot 的 Rust 原生加速扩展（PyPI 包名 `moviepilot-rust`，导入名 `moviepilot_rust`），默认开发分支为 `v3`，支持标准 Python `3.14` 与自由线程 Python `3.14t`。构建与模块结构说明见 [README.md](README.md)。

## 架构与并发边界

- **纯 Rust 业务核心与薄绑定分离** `src/filter/`、`indexer/`、`metainfo/`、`metamusic/`、`rss/` 与 `support/` 只接收和返回纯 Rust 数据结构，不持有 Python 对象引用，也不耦合宿主配置、数据库或网络 I/O。`src/bindings/` 统一负责 PyO3 输入快照提取、输出转换并在进入重计算前释放 GIL。
- **确定性与异常安全** 正则编译、HTML/XML 流式解析与标题切分对任意畸形外部输入均须安全返回或降级（如无效用户自定义正则按未匹配处理），禁止在面向宿主调用的路径上因外部输入触发未捕获 `panic!`。
- **自由线程（`cp314t`）与缓存安全** 所有跨调用共享缓存（如基于 `Arc` 的有界解析选项缓存）必须线程安全，不得引入未同步的全局可变状态或假设 GIL 隐式互斥。公开结构体、绑定函数签名与复杂解析状态机补充简明中文注释。

## 本地验证与发布边界

提交 PR 前须通过 Rust 与 Python 双侧检查

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
python -m maturin develop
python -m pytest -q
```

- 改动触及 PyO3 绑定、条件编译特性（如 `zhconv`）或并发共享状态时，须按 `.github/workflows/ci.yml` 同时验证标准 Python `3.14` 与自由线程 `3.14t`（`--no-default-features --features zhconv`）构建及测试。
- 使用单行英文 Conventional Commits。除非当前任务明确执行版本发布，不得修改 `Cargo.toml` 中的 `version`（`v3` 分支上的版本号变更会直接触发自动发版与 PyPI 上传）。
