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

자세한 내용은 [README.md](README.md)에 있습니다.

## 릴리스

릴리스는 `.github/workflows/sign-and-publish.yml`이 세 빌드에 서명해 공개하는 것입니다. 순서는 그 파일 머리말에 있습니다. 공개되는 자산 이름은 Redrob Launcher가 읽는 형식 `redrob-shape-<버전>-<os>-<arch>[-portable].<확장자>` 하나뿐이며, `scripts/check-asset-names.py`가 업로드 직전에 그 이름을 검사합니다. 릴리스에는 런처용 매니페스트 `redrob-launcher.json`(`{ "schema": 1, "sharedAccount": true }`)도 함께 올라가며, 같은 검사가 그 파일이 없는 릴리스를 막습니다. 이 파일은 이 릴리스가 런처의 공유 계정 파일을 읽는다는 뜻이라, 런처에 로그인하는 것만으로 충분합니다. Linux 빌드는 `scripts/package-linux.sh <설치 디렉터리>`로 묶습니다.
