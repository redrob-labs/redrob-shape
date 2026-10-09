# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Korean for the Redrob Shape sidebar, through Blender's own translation system.

Blender shows these when Preferences > Interface > Translation is set to Korean. Fixed labels are
translated by Blender itself; text built at run time goes through ``iface_`` with ``{}`` holes, so
the hole stays in the message id. Product names stay as they are in both languages: they are the
names the user meets in Redrob Console and in file names. tests/test_i18n.py checks that every
string the sidebar shows has a Korean entry.
"""

import bpy

KO = {
    # Status
    "Ready": "준비됨",
    "Thinking": "생각하는 중",
    "Review Plan": "계획 검토",
    "Applying": "적용하는 중",
    "Applied": "적용됨",
    "Needs Attention": "확인 필요",
    "Status": "상태",
    "Connected": "연결됨",
    "Ready for a new instruction": "새 요청을 기다리는 중",
    "Connected • you review every plan before it changes anything": "연결됨 • 장면을 바꾸기 전에 모든 계획을 검토합니다",
    "Connect Redrob Console to create plans": "계획을 만들려면 Redrob Console을 연결하세요",
    "Planning • no scene changes yet": "계획 만드는 중 • 아직 장면은 그대로입니다",
    "Plan ready • switch off any step you do not want, then apply": "계획 준비됨 • 원하지 않는 단계는 끄고 적용하세요",
    "Request cancelled": "요청을 취소했습니다",
    "Request is no longer active": "이미 끝난 요청입니다",
    "Cancelling request": "요청을 취소하는 중",
    "Rejected plan: {}": "계획을 거부했습니다: {}",
    "The scene changed while planning; create a new plan": "계획하는 동안 장면이 바뀌었습니다. 새 계획을 만드세요",
    "The scene changed after planning; create a new plan": "계획한 뒤 장면이 바뀌었습니다. 새 계획을 만드세요",
    "Local agent exited before completing the request": "로컬 에이전트가 요청을 끝내기 전에 멈췄습니다",
    "Online access is disabled in Blender preferences": "Blender 환경설정에서 온라인 접근이 꺼져 있습니다",
    "Enter an instruction first": "먼저 요청을 입력하세요",
    "Select what to shape, or switch to Whole Scene": "다듬을 대상을 선택하거나 장면 전체로 바꾸세요",
    "Switch on at least one step": "단계를 하나 이상 켜세요",
    "Apply stopped at {}. Use Undo to roll back.": "{}에서 적용이 멈췄습니다. 실행 취소로 되돌리세요.",
    "Applied {} steps • look at the result, then keep or undo it": "{}단계를 적용했습니다 • 결과를 보고 유지하거나 되돌리세요",
    "Undone • change the steps and apply again, or describe a change": "되돌렸습니다 • 단계를 바꿔 다시 적용하거나 바꿀 점을 말하세요",
    "Kept • describe the next change": "유지했습니다 • 다음에 바꿀 점을 말하세요",
    # Prompt
    "What should change?": "무엇을 바꿀까요?",
    "Refine it, or ask for something new": "더 다듬거나 새로 요청하세요",
    "Work on": "대상",
    "Selected": "선택한 것",
    "Whole Scene": "장면 전체",
    "Shape only the selected objects": "선택한 오브젝트만 다듬습니다",
    "Let the plan touch any object in the scene": "장면의 모든 오브젝트를 다룰 수 있습니다",
    "Create Plan": "계획 만들기",
    "Try Again": "다시 시도",
    "Instruction": "요청",
    "Describe the result you want in the current scene": "지금 장면에서 원하는 결과를 설명하세요",
    # Plan review
    "Proposed changes": "바꿀 내용",
    "{} of {} steps": "{}/{}단계",
    "Switch off any step you do not want, then apply": "원하지 않는 단계는 끄고 적용하세요",
    "Include": "포함",
    "Include this step when applying": "적용할 때 이 단계를 포함합니다",
    "Details": "자세히",
    "Show every value of this step": "이 단계의 값을 모두 봅니다",
    "Invalid action arguments": "잘못된 단계 값",
    "Look at the result in the viewport, then keep or undo it": "뷰포트에서 결과를 보고 유지하거나 되돌리세요",
    "Apply": "적용",
    "Apply Plan": "계획 적용",
    "Keep": "유지",
    "Undo": "실행 취소",
    "Discard": "버리기",
    "Cancel": "취소",
    "Yes": "예",
    "No": "아니요",
    "Edits the mesh itself": "메시를 직접 바꿉니다",
    "Bakes the modifier into the mesh": "모디파이어를 메시에 굳힙니다",
    "Removes the object. Undo brings it back": "오브젝트를 지웁니다. 실행 취소로 되살립니다",
    "Creates a new single-user material": "이 오브젝트만 쓰는 새 재질을 만듭니다",
    # Step names
    "Add": "추가",
    "Move": "변형",
    "Boolean": "불리언",
    "Extrude": "돌출",
    "Inset": "인셋",
    "Subdivide": "세분화",
    "Shade": "셰이딩",
    "Apply Modifier": "모디파이어 적용",
    "Duplicate": "복제",
    "Delete": "삭제",
    "Rename": "이름 바꾸기",
    "Material": "재질",
    "Modifier": "모디파이어",
    "Bevel": "베벨",
    "Subdivision": "서브디비전",
    "Solidify": "두께",
    "Mirror": "미러",
    "Array": "배열",
    "Twist": "비틀기",
    "Bend": "휘기",
    "Taper": "테이퍼",
    "Stretch": "늘이기",
    "Noise": "노이즈",
    "Smooth": "스무스",
    # Step values
    "Object": "오브젝트",
    "Name": "이름",
    "Primitive": "기본 도형",
    "Location": "위치",
    "Rotation": "회전",
    "Scale": "크기",
    "Width": "너비",
    "Segments": "세그먼트",
    "Levels": "단계",
    "Render Levels": "렌더 단계",
    "Thickness": "두께",
    "Axis": "축",
    "Count": "개수",
    "Offset": "간격",
    "Angle": "각도",
    "Factor": "정도",
    "Strength": "세기",
    "Noise Scale": "노이즈 크기",
    "Iterations": "반복",
    "With": "상대",
    "Operation": "연산",
    "Hide Cutter": "자르는 오브젝트 숨기기",
    "Faces": "면",
    "Distance": "거리",
    "Depth": "깊이",
    "Cuts": "자르기 수",
    "Smoothness": "부드러움",
    "Base Color": "기본 색",
    "Metallic": "금속성",
    "Roughness": "거칠기",
    # Chat
    "History": "기록",
    "Undone": "되돌림",
    "{} steps": "{}단계",
    "You": "나",
    "Send": "보내기",
    "Thinking… {} s": "생각하는 중… {}초",
    "Tell me what to change. You see every step before anything in the scene changes.": "무엇을 바꿀지 말해 주세요. 장면이 바뀌기 전에 모든 단계를 먼저 보여 드립니다.",
    "Try: “bevel the cube and smooth it”": "예: “큐브 모서리를 둥글게 하고 부드럽게 해 줘”",
    "Applied: {}": "적용됨: {}",
    "Applied again": "다시 적용됨",
    "Kept": "유지함",
    "Plan discarded": "계획을 버렸습니다",
    "Asked again for a different plan": "다른 계획을 다시 요청했습니다",
    "Show {} earlier messages": "이전 메시지 {}개 보기",
    "Show Fewer": "줄여 보기",
    "Show Earlier Messages": "이전 메시지 보기",
    "Show the whole conversation, not only the newest messages": "최근 메시지만이 아니라 대화 전체를 봅니다",
    "Wide View": "넓게 보기",
    "Open the chat in the Shape workspace tab, with a wide sidebar": "넓은 사이드바가 있는 Shape 작업 공간 탭에서 대화를 엽니다",
    "Could not add the Shape workspace": "Shape 작업 공간을 만들지 못했습니다",
    "New Conversation": "새 대화",
    "Forget earlier requests and start over": "앞선 요청을 잊고 새로 시작합니다",
    "Use Again": "다시 쓰기",
    "Put this earlier instruction back in the prompt": "이 요청을 입력란에 다시 넣습니다",
    # Operators
    "Ask Redrob to prepare a plan you review before anything changes": "바꾸기 전에 검토할 계획을 요청합니다",
    "Ask for a different plan for the same instruction": "같은 요청으로 다른 계획을 받습니다",
    "Cancel the current Redrob request": "지금 요청을 취소합니다",
    "Apply the switched-on steps as one undoable Blender operation": "켜 둔 단계를 실행 취소 한 번으로 되돌릴 수 있게 적용합니다",
    "Keep the applied result and continue the conversation": "적용한 결과를 유지하고 대화를 이어 갑니다",
    "Throw the plan away without applying it": "적용하지 않고 계획을 버립니다",
    # Footer and sub-panels
    "Console": "Console",
    "Reconnect": "다시 연결",
    "Scene Context": "장면 정보",
    "Active": "활성",
    "Mode": "모드",
    "None": "없음",
    "Geometry, images, files and paths stay local": "형상, 이미지, 파일, 경로는 이 컴퓨터에 남습니다",
    "Appearance": "모양",
    "Native Blender UI with Redrob Shape styling": "Redrob Shape 스타일을 입힌 Blender 기본 화면",
    "Restore Previous": "이전으로 되돌리기",
    "Apply Modern Theme": "새 테마 적용",
    "Redrob Shape": "Redrob Shape",
    "Reconnect Agent": "에이전트 다시 연결",
    "Restart the local agent and re-check Redrob configuration": "로컬 에이전트를 다시 시작하고 설정을 다시 확인합니다",
    "Apply Redrob Shape Theme": "Redrob Shape 테마 적용",
    "Apply the modern Redrob Shape visual preset to Blender's native UI": "Blender 기본 화면에 Redrob Shape 테마를 입힙니다",
    "Restore Previous Theme": "이전 테마로 되돌리기",
    "Restore the theme values captured before applying Redrob Shape": "Redrob Shape 테마를 적용하기 전 값으로 되돌립니다",
    "Open Redrob Console": "Redrob Console 열기",
    "Open Redrob Console in the default browser": "기본 브라우저에서 Redrob Console을 엽니다",
}

#: Blender looks a label up in "*" and an operator's text in "Operator"; register both.
_CONTEXTS = ("*", "Operator")


def iface_(message):
    return bpy.app.translations.pgettext_iface(message)


def translations():
    # Blender names the locale ko_KR; "ko" covers a build that names it by language alone.
    table = {(context, message): text for context in _CONTEXTS for message, text in KO.items()}
    return {"ko_KR": table, "ko": dict(table)}


def register():
    bpy.app.translations.register(__package__, translations())


def unregister():
    bpy.app.translations.unregister(__package__)
