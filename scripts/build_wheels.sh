#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYTHON_BIN="${PYTHON_BIN:-${ROOT_DIR}/.venv/bin/python}"
OUT_DIR="${OUT_DIR:-${ROOT_DIR}/dist}"

# 构建当前平台 wheel 和源码包到 dist 目录。
build_local_artifacts() {
  if [[ ! -x "${PYTHON_BIN}" ]]; then
    echo "未找到项目虚拟环境 Python: ${PYTHON_BIN}" >&2
    echo "请先在项目根目录创建 .venv，并安装 maturin。" >&2
    exit 1
  fi

  mkdir -p "${OUT_DIR}"
  rm -f "${OUT_DIR}"/moviepilot_rust-*.whl "${OUT_DIR}"/moviepilot_rust-*.tar.gz
  "${PYTHON_BIN}" -m maturin sdist --out "${OUT_DIR}"
  "${PYTHON_BIN}" -m maturin build --release --out "${OUT_DIR}"
}

build_local_artifacts "$@"
