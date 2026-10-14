# 레드롭 셰이프

레드롭 셰이프는 프롬프트로 모델을 만들고 셰이딩하며, 기존 .blend 파일을 여는 에이전트형 3D 제작 도구입니다.

[English](README.md)

처음부터 끝까지 순수 Rust로 만듭니다. 하프에지 모델러, wgpu 뷰포트, 경로 추적기, egui 에디터, 그리고
수정안을 제안하고 승인을 기다리는 내장 에이전트로 이루어집니다. Blender도, C++ 툴체인도, OS별 빌드 머신도
없습니다. Linux·macOS·Windows 모두 `cargo build` 한 번이 빌드 전부입니다.

## 상태

초기 단계입니다. 단계 계획은 [ROADMAP.md](ROADMAP.md)에 있고, 지금 동작하는 범위는 그 표가 기준입니다.

## 빌드

```
cargo build --release -p redrob-shape
./target/release/redrob-shape
```

Linux는 창 관련 헤더가 필요합니다 (Debian/Ubuntu: `libxkbcommon-dev libwayland-dev libx11-dev
libxrandr-dev libxi-dev libgl1-mesa-dev`).

## 개발

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

브랜치는 gitflow를 따릅니다. `develop`이 기본, `main`이 릴리즈, 작업 브랜치는 `feat/…`, `fix/…`,
`docs/…`, `chore/…`, `test/…`, `refactor/…`, `perf/…`, `hotfix/…`, `release/…`, `sync/…` 입니다.
조직 공통 [CONTRIBUTING](https://github.com/redrob-labs/.github/blob/main/CONTRIBUTING.ko.md)을 참고하세요.

## 이력

2026년 10월까지 이 저장소는 패치 기반 Blender 배포판이었습니다. 그 트리는 태그 `blender-fork-final`과
브랜치 `archive/blender-fork-history`에 남아 있고, 여기에는 그중 아무것도 가져오지 않았습니다.

## 라이선스

MIT 또는 Apache-2.0 중 택일. [NOTICE](NOTICE) 참고.
