# 레드롭 셰이프

레드롭 셰이프는 프롬프트로 모델을 만들고 셰이딩하며, 기존 .blend 파일을 여는 에이전트형 3D 제작 도구입니다.

[English](README.md)

처음부터 끝까지 순수 Rust로 만듭니다. 하프에지 모델러, wgpu 뷰포트, 경로 추적기, egui 에디터, 그리고
수정안을 제안하고 승인을 기다리는 내장 에이전트로 이루어집니다. Blender도, C++ 툴체인도, OS별 빌드 머신도
없습니다. Linux·macOS·Windows 모두 `cargo build` 한 번이 빌드 전부입니다.

## 상태

초기 단계입니다. 단계 계획은 [ROADMAP.md](ROADMAP.md)에 있고, 지금 동작하는 범위는 그 표가 기준입니다.

## 스크린샷


| | |
|---|---|
| ![Layout 워크스페이스](docs/screenshots/layout.png)<br>Layout 워크스페이스 | ![추가 메뉴 (Shift+A)](docs/screenshots/add-menu.png)<br>추가 메뉴 (Shift+A) |
| ![편집 모드](docs/screenshots/edit-mode.png)<br>편집 모드 | ![모디파이어](docs/screenshots/modifiers.png)<br>모디파이어 |
| ![아웃라이너](docs/screenshots/outliner.png)<br>아웃라이너 | ![셰이더 편집기](docs/screenshots/shader-editor.png)<br>셰이더 편집기 |
| ![셰이딩 팝오버](docs/screenshots/shading-popover.png)<br>셰이딩 팝오버 | ![스컬프트](docs/screenshots/sculpt.png)<br>스컬프트 |
| ![UV 편집](docs/screenshots/uv-editing.png)<br>UV 편집 | ![애니메이션](docs/screenshots/animation.png)<br>애니메이션 |
| ![포즈 모드](docs/screenshots/pose-mode.png)<br>포즈 모드 | ![웨이트 페인트](docs/screenshots/weight-paint.png)<br>웨이트 페인트 |
| ![렌더 (패스 트레이서)](docs/screenshots/render.png)<br>렌더 (패스 트레이서) | ![커브](docs/screenshots/curves.png)<br>커브 |
| ![COLLADA 가져오기](docs/screenshots/collada-import.png)<br>COLLADA 가져오기 | ![지오메트리 노드](docs/screenshots/geometry-nodes.png)<br>지오메트리 노드 |
| ![환경설정 / 키맵](docs/screenshots/preferences.png)<br>환경설정 / 키맵 | ![스크립팅 콘솔](docs/screenshots/scripting-console.png)<br>스크립팅 콘솔 |

## 설치

릴리즈는 Linux(tar.gz), macOS(공증된 DMG), Windows(`-setup.exe` 사용자 설치기 또는 아무 데나 풀어
쓰는 `-portable.zip`)로 나옵니다. Redrob Launcher는 portable 빌드를 설치합니다.

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

## 명령줄

같은 실행 파일이 창을 열지 않고 씬 파일을 다룹니다.

```bash
redrob-shape convert scene.blend scene.glb       # 읽을 수 있는 형식을 쓸 수 있는 형식으로
redrob-shape info scene.rshape                   # 씬 요약을 JSON으로
redrob-shape tools                               # 도구 목록을 JSON으로
redrob-shape tool --in a.rshape --out a.rshape add_primitive '{"kind":"cube","size":2}'
redrob-shape run --in a.rshape --out b.glb build.shape   # Shape Script 실행, -는 표준 입력
redrob-shape mcp --headless --in a.rshape --save a.rshape # 창 없이 stdio MCP
```

종료 코드는 0 성공, 1 명령 실패, 2 사용법 오류입니다.

## Redrob Code에서 제어하기

Redrob Shape 창이 열려 있는 동안 창은 공용 Redrob Code 데몬에 자신을 등록합니다. 그래서 어느
Redrob 표면의 에이전트든 이 창을 MCP 서버 `app:shape`로 보고, 사용자가 보고 있는 그 씬을 조작할 수
있습니다. 모든 호출은 UI 스레드에서 메뉴와 같은 코드로 실행되고, 편집은 실행 취소 이력에 남습니다.
데몬이 없으면 앱은 평소대로 동작합니다.

- `scene_info` -- 파일 경로(또는 제목 없음), 오브젝트 수, 선택, 뷰포트·씬 카메라, 렌더 크기.
- `open_scene {path}` -- 씬 파일을 창에 열어 현재 씬을 교체합니다.
- `list_objects {max?}` -- 오브젝트마다 한 줄: id, 이름, 종류, 부모, 표시/숨김.
- `select {ids}` -- id 또는 이름으로 오브젝트를 선택합니다(기존 선택을 교체).
- `transform {id, translate?, rotate_deg?, scale?}` -- 오브젝트 하나를 현재 위치 기준으로 이동·회전·크기 조절합니다.
- `set_visible {id, visible}` -- 오브젝트 하나를 표시하거나 숨깁니다.
- `render {path?, width?, height?}` -- 렌더 패널 설정으로 렌더해 PNG로 쓰고 경로를 돌려줍니다.
- `save` -- 씬을 원래 파일에 저장합니다. 제목 없는 씬이면 실패합니다.

## 이력

2026년 10월까지 이 저장소는 패치 기반 Blender 배포판이었습니다. 그 트리는 태그 `blender-fork-final`과
브랜치 `archive/blender-fork-history`에 남아 있고, 여기에는 그중 아무것도 가져오지 않았습니다.

## 라이선스

MIT 또는 Apache-2.0 중 택일. [NOTICE](NOTICE) 참고.
