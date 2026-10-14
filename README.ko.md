# Redrob Shape: Blender를 패치로 고친 배포판

Redrob Shape는 말로 모델을 다듬고 셰이딩하는 에이전트형 3D 도구이며, 지금 쓰는 .blend 파일을 그대로 엽니다.

[English](README.md)

이 저장소에는 **우리가 바꾼 것만** 있습니다. Blender 원본은 빌드할 때 받아 오고, 저장소에 넣지 않습니다.

```
upstream.json     고정한 Blender 저장소와 커밋
overlay/          이 제품에만 있는 파일 (복사해 넣고, 패치하지 않음)
patches/          Blender 파일에 대한 변경, 목적마다 패치 하나
brand/            디자인 시스템 파일로 만든 아이콘과 스플래시 (sha256으로 고정)
scripts/          받기, 적용, 게이트
```

빌드와 검사:

```
./scripts/verify-patches.sh     # 원본을 받아 모든 변경을 적용하고 결과를 검사합니다
```

아이콘과 스플래시는 `scripts/build-brand-assets.py`가 디자인 시스템이 배포한 파일로 만듭니다. 여기서 그리지 않습니다. Redrob Shape는 일곱 제품군에 속하지 않으므로 그룹 아이콘(Redrob Blue 타일 위 흰 심볼)을 씁니다.

## Redrob Code가 필요합니다

계획은 이 컴퓨터의 모든 Redrob 앱이 함께 쓰는 엔진 **Redrob Code**를 거칩니다(사용자마다 `redrob serve --register` 하나). 사이드카가 엔진의 기록 파일로 엔진을 찾고, 떠 있지 않으면 띄우고, 모든 계획 요청을 엔진의 OpenAI 호환 `/v1/chat/completions`에 `x-redrob-app: shape`와 Blender 세션마다 하나인 `x-redrob-session`을 붙여 보냅니다. Redrob Console 키는 엔진이 보관하고 마스킹도 엔진이 합니다. 애드온과 사이드카는 키를 갖지 않으며 Console로 가는 직접 경로도 없습니다. Blender에서 연결하면 Console의 기기 코드 흐름으로 받은 키를 엔진에 넘기고, `redrob providers login`이나 Redrob Launcher의 로그인도 모든 앱에 같은 방식으로 적용됩니다.

Redrob Code가 설치되어 있지 않으면 사이드바가 그 사실을 알리고, 엔진을 설치하는 [Redrob Launcher](https://github.com/redrob-labs/redrob-launcher/releases/latest)를 안내합니다. 이전 Redrob Shape가 OS 키체인에 두었던 키는 첫 실행 때 엔진으로 옮기고 키체인에서 지웁니다. 프로토콜과 오류 코드는 사이드카의 [README](overlay/intern/redrob_agent/README.md)에 있습니다.

자세한 내용은 [README.md](README.md)에 있습니다.

## 릴리스

릴리스는 `.github/workflows/sign-and-publish.yml`이 세 빌드에 서명해 공개하는 것입니다. 순서는 그 파일 머리말에 있습니다. 공개되는 자산 이름은 Redrob Launcher가 읽는 형식 `redrob-shape-<버전>-<os>-<arch>[-portable].<확장자>` 하나뿐이며, `scripts/check-asset-names.py`가 업로드 직전에 그 이름을 검사합니다. 릴리스에는 런처용 매니페스트 `redrob-launcher.json`(`{ "schema": 1, "sharedAccount": true }`)도 함께 올라가며, 같은 검사가 그 파일이 없는 릴리스를 막습니다. 이 파일은 런처에 로그인하는 것만으로 이 릴리스에 충분하다는 뜻입니다. 런처의 로그인은 Redrob Code를 거쳐 Redrob Shape에 닿습니다(엔진이 자기 키가 없을 때 공유 계정 파일을 읽습니다). Linux 빌드는 `scripts/package-linux.sh <설치 디렉터리>`로 묶습니다.
