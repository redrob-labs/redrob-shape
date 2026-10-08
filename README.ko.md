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
