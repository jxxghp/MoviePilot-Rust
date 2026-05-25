# MoviePilot-Rust

MoviePilot Rust 加速模块。

- PyPI 包名：`moviepilot-rust`
- Python 导入名：`moviepilot_rust`
- 支持版本：Python 3.11+
- 构建工具：PyO3 + maturin

## 本地开发安装

在本仓库根目录创建 `.venv`，并把 Rust 扩展直接安装到这个虚拟环境：

```shell
python3 -m venv .venv
.venv/bin/python -m pip install "maturin>=1.9,<2"
.venv/bin/python -m maturin develop --release
```

验证扩展是否可导入：

```shell
.venv/bin/python -c "import moviepilot_rust; print(moviepilot_rust.is_available())"
```

输出 `True` 表示本地编译安装成功。

## 本地打包

使用仓库内脚本构建当前平台 wheel 和源码包：

```shell
scripts/build_wheels.sh
```

产物会输出到 `dist/`，例如：

```text
dist/moviepilot_rust-0.1.4-cp311-abi3-macosx_11_0_arm64.whl
dist/moviepilot_rust-0.1.4.tar.gz
```

脚本默认使用本仓库 `.venv/bin/python`。

## 在 MoviePilot 主项目中使用

主项目通过普通 pip 依赖使用本模块，不再从主项目源码内编译 Rust。

以下命令在 MoviePilot 主项目根目录执行。

安装 PyPI 版本：

```shell
.venv/bin/pip install moviepilot-rust
```

安装本地刚构建的 wheel：

```shell
.venv/bin/pip install --force-reinstall ../MoviePilot-Rust/dist/moviepilot_rust-*.whl
```

在 MoviePilot 主项目里验证运行时状态：

```shell
.venv/bin/python -c "from app.utils import rust_accel; print(rust_accel.status())"
```

`available=True` 表示扩展已安装可用，`enabled=True` 表示 MoviePilot 当前配置开关允许使用 Rust 加速。

## 发布

在 GitHub 上 **Publish Release** 会触发 Actions，自动构建 Linux（glibc/musl）、macOS、Windows wheel 并发布到 PyPI；发布成功后还会向 [MoviePilot](https://github.com/jxxghp/MoviePilot) 的 `v2` 分支提交 bump `requirements.in` 的 PR（需配置 `MOVIEPILOT_REPO_TOKEN`）。

```shell
git tag v0.1.4
git push origin v0.1.4
```

然后在 GitHub 仓库 **Releases → Draft a new release** 中选择上述 tag 并点击 **Publish release**。仅推送 tag 不会触发构建。
