<a id="direct-context-editing"></a>
<a id="helper-and-normalization-contracts"></a>
<a id="independent-operation-bundles"></a>
<a id="ownership-and-activation"></a>
<a id="streaming-replay-and-versions"></a>
<a id="validation"></a>
<a id="editing-compatibility-contract"></a>

# 편집 호환 계약

[English](../editing-design.md) | [한국어](editing-design.md)

이 설계는 선택형 편집 호환 기능을 정의한다. 여기서 설명하는 편집 정책은 아직
런타임에 제공되지 않는다. 기존 custom 문자열 브리지가 기본값이다. 합성 fixture는
고정 Codex 0.154.0의 직접 패치와 Code Mode helper 실행을 검증하며 실제 공급자를
검증하지 않는다.

## 책임과 활성화

호스트가 검증된 클라이언트 계약을 선택한다. 모델은 `editing_policies`에서
`editing_policy`를 명시적으로 선택하며 정의만으로 활성화되지 않는다. CLI와
Rust 라우터 구성은 동일하게 검증된 설정을 사용한다. 게이트웨이는 표현 변환과
검증을 담당한다. Codex가 권한·승인·파일 접근·실제 적용을 소유한다. 새 편집
프로세스·파일 실행기·자동 재시도·공급자 이름 추론은 추가하지 않는다.

정책은 `version`, `client_contract`, `representation`, `patch_dialect`,
`normalization`을 지정한다. 정책이 없으면 기존 동작을 유지한다. 선언과 실제
요청은 일치해야 한다. 편집 도구가 없는 요청에 도구를 추가하지 않는다. 미지 계약은
전송 전에 거부한다. 이름을 지정한 강제 선택에는 합성 대안을 추가하지 않는다.
기존 이력을 위해 원래 도구도 유지한다.

## 직접 문맥 편집

`codex-direct-custom/v1`은 고정 custom 패치 계약이며 `context-lines/v1`은
한 파일·한 문맥 변경이다. 필수 필드는 `path`, `before_context`, `old_lines`,
`new_lines`, `after_context`다. 줄 배열은 줄바꿈 없는 문자열을 담는다. 미지·중복
키, 무변경·빈 편집, 구분자 주입과 개행만의 변경은 거부한다. 공백과 Unicode는
보존한다. 컴파일러는 결정적인 `codex-patch/1` 패치를 생성하고 등록 문법으로
검증한다. 실제 파일 문맥 일치는 호스트가 판단한다.

합성 함수 호출 하나는 원래 custom 호출 하나에 대응한다. 요청 registry는 충돌 없는
이름·namespace·선택·호출과 결과 연결을 소유한다. 정규 패치는 문맥 입력으로
왕복하며 표현할 수 없는 기존 패치는 원래 도구 경로로 유지한다. 관리형 replay는
새 매핑 메타데이터를 포함하여 공급자 원본 호출과 공개 복원 호출을 인증해야 한다.

## Helper와 정규화 계약

`codex-code-mode/v1`은 고정 custom exec 선언과 호스트가 선택한 helper 계약을
요구한다. 패치를 JSON 문자열로 직렬화해 고정 helper wrapper 하나를 만든다.
JavaScript를 실행하거나 일반적으로 해석하지 않는다. 생성한 wrapper만 역변환하며
임의 프로그램·주석·내부 패치 문자열은 유지한다. Exec 출력은 전체 프로그램의
결과이며 helper 성공을 만들어내지 않는다. 미등록 최상위 호출은 보정하지 않는다.

기본값은 `normalization=none`이다. 후속 명시적 envelope 규칙은 완성된 패치
하나의 바깥 시작·종료 줄에 붙은 추가 구분자만 제거할 수 있다. 본문·경로·환경
선택을 바꾸지 않고 정규화 후 문법을 다시 검사한다. 코드 fence·설명·미완성
패치·다른 도구는 복구하지 않는다.

## 독립 작업 묶음

후속 `operations/v1` 표현은 순서 있는 생성·삭제·이동·문맥 수정이다. 묶음은
패치 하나와 실행 결과 하나가 된다. 반복 경로·이동 의존·확인 가능한 어휘적 경로
별칭은 거부한다. 호스트 근거 없이 inode 동일성·원자성·rollback·파일별 성공을
추론하지 않는다. 앞선 변경에 의존하는 순차 편집은 범위 밖이다.

## 스트리밍·재생·버전

편집 인자는 기존 한도 안에서 변환·검증 완료까지 보관한다. 선행 텍스트 진행과
항목 순서는 유지한다. continuation 확정과 durable recorder 최종 ACK 전에
실행 가능한 완료를 공개하지 않는다. EOF·취소·실패는 완료나 추론 재실행을 만들지 않는다.

정책 의미에는 `gateway-editing-policy/v1`, 선택형 실행에는
`gateway-embedded-manifest/v7`, `gateway-ready/v7`,
`gateway-extended-manifest/v7`, `gateway-extended-ready/v7`을 예약한다.
새 codec·팩·인증된 매핑에는 `gateway-api-codec/v2`, `gateway-profile-pack/v2`,
`gateway-continuation/v3`을 예약한다. 이는 계획 버전이며 현재 지원 스키마가 아니다.
기존 설정과 v1/v2 replay는 원래 바이트 계약을 유지한다. DB 테이블 이행이나 암호문
재작성은 자동 수행하지 않는다. 새 경로 origin에는 선택한 정책·구현 버전을 모두
연결한다. rollback에는 호환되는 바이너리·정책·패키지 선택·DB·키·호스트 이력이 필요하다.

## 검증

소스 시험 `tests/codex/editing_contract.py`는 `--gateway-bin`과 `--runtime-dir`을
받는다. 런타임 바이트를 검증하고 loopback 합성 공급자로 직접·helper 호출의
적용과 승인 거부를 확인한다. 공개 fixture에는 계약 hash와 합성 메타데이터만
있으며 런타임 프롬프트는 복사하지 않는다. 구현 시 내장·외부 codec, 인라인·가져온
정책, stateless·managed 이력은 같은 공개 계약을 보존해야 한다.
