<a id="g15--continuity-with-host-owned-history"></a>

# G15 — 호스트 이력에 기반한 연속성

[English](../continuity-design.md) | [한국어](continuity-design.md)

상태: **2026-09-08 명시적으로 승인; 구현과 수락은 G16/G17에서 추적**.

<a id="evidence-and-recommendation"></a>

## 근거와 권고

Gateway는 저장된 response ID와 원격 compact endpoint를 거부한다. 이것만으로
장기 Codex 작업에 gateway 데이터베이스가 필요해지지는 않는다. 고정 Codex는
설정한 provider가 원격 압축 미지원일 때 로컬 압축을 선택한다. 합성 제어 시험은
초기 turn, `thread/compact/start`, 후속 turn을 일반 `/v1/responses` 세 번으로
완료했으며 gateway compact endpoint가 필요하지 않았다.

같은 세 요청의 로컬 압축 시험은 별도 승인된 임시 0.154.0-alpha.6에서도 통과했다.

출처: [고정된 압축 작업 선택](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/tasks/compact.rs).
이 증거는 시험한 custom-provider 프로필에 해당한다. 모든 모델·프로필이나
실제 모델 요약의 충분성을 증명하지 않는다.

**Strongly Recommended, 책임 배치의 신뢰도 높음:** Codex·호스트가 이력을,
gateway가 stateless 전송을 소유하도록 유지한다. 로컬 압축과 전체 이력 재입력을
먼저 검증한다. 관측한 경로에는 새 gateway 이력 DB, provider ID 에뮬레이션이나
암호화 요약 envelope가 필요하지 않으므로 초기 구현에서 제외한다.

<a id="responsibilities-and-interfaces"></a>

## 책임과 인터페이스

| 소유자 | 책임 |
|---|---|
| Codex | Thread 이력, 로컬 압축, 도구 결과 맥락과 재개 제어 프로토콜 |
| 호스트 런타임 | 검증된 실행 파일·설정, 실행 binding, 취소·복구·명시적 모델 전환 |
| Gateway | 요청별 고정 경로, 공급자 자격 선택, 프로토콜 변환과 stateless 오류 |
| 소비자 | 워크플로 checkpoint, 승인, 업무 의미와 운영 수락 |

기존 `thread/start`, `thread/resume`, `thread/compact/start`, `turn/start`,
`turn/interrupt`를 사용한다. 이 설계는 gateway 저장, 응답 조회나 compact HTTP
endpoint를 활성화하지 않는다. 미지원 상태 요청은 존재하지 않는 공급자 ID로
전달하지 않고 계속 실패시킨다.

<a id="run-binding-and-recovery-record"></a>

## 실행 binding과 복구 기록

호스트는 기존 실행 기록 옆에 비공개 버전 기록 `gateway-run-binding/v1`을 저장한다.

- Codex 버전, 실행 파일 digest와 선언된 상태 호환성.
- Gateway 버전·digest와 정확한 유효 설정 digest.
- 해석된 공급자·실제 모델·API·프로필·어댑터 버전.
- 자격 증명 realm과 세대 참조; 원문 키는 기록하지 않음.
- Thread 정체성, 비공개 이력 참조, 마지막 완료 turn과 복구 상태.
- 컨텍스트·출력 한도, 압축 정책과 실행의 재시도·요청 예산.

전이가 완료되면 새 기록을 원자적으로 저장하고 이전 기록과 호환 이력 백업을
유지한다. 완료 실행 증거를 덮어쓰거나 모델 응답에서 워크플로 승인을 추론하지 않는다.

재개 전에 실행 파일, 상태 호환성, 경로·프로필과 자격 증명 세대를 확인한다.
세대는 권위 있는 자격 증명 소유자에서 얻어야 한다. 같은 환경 변수 이름을
다시 쓰는 것만으로는 부족하다. Binding을 확인할 수 없으면 같은 맥락의 재개를
거부하고 변경된 설정으로 과거 별칭을 조용히 다시 해석하지 않는다.

<a id="supported-continuation-paths"></a>

## 지원하는 연속 실행 경로

1. **같은 프로세스·경로:** Codex가 현재 전체 맥락과 도구 결과를 전송한다.
   Gateway는 선언된 stateless 부분집합만 변환한다.
2. **로컬 압축:** 해석된 remote-compaction 기능이 Unsupported인 검증 프로필을
   사용한다. Codex가 요약 요청과 대체 이력을 소유한다. Gateway는 일반 Responses
   요청을 처리하며 요약을 native 암호화 상태로 꾸미지 않는다.
3. **프로세스 재시작:** 호환 Codex HOME·이력과 일치하는 binding을 복원한다.
   기록한 thread를 재개하고 모델·공급자를 확인한 뒤 새 turn을 시작한다.
   이미 완료한 도구는 재실행하지 않는다.
4. **명시적 모델 변경:** 호스트 전이와 새로 검증한 경로를 요구한다. 이동 가능한
   메시지·완료 도구 결과로 새 맥락을 만들고 생략한 opaque state와 새 binding을
   기록한다. 승인·워크플로 checkpoint는 기존 소유자에 보존한다.

원격 압축을 요구하는 provider 프로필은 이 로컬 계약에 사용할 수 없다. 별도
어댑터 설계·승인 전까지 미지원이다. 새 Codex 버전은 실제 어떤 경로를 사용하는지
다시 증명해야 하며 표시 이름이나 과거 관측에만 의존하지 않는다.

<a id="opaque-state-cancellation-and-uncertainty"></a>

## 불투명 상태·취소·불확실성

Native opaque state는 동일하게 검증된 origin에서만 유지한다. Cross-protocol
opaque 재사용은 오류다. Reasoning 요약은 명시적으로 그렇게 표현됐을 때만
일반 이동 가능한 내용이며 서명·암호화 공급자 상태를 대신하지 않는다. 새 암호화
envelope나 영속 키 관리 체계는 도입하지 않는다.

취소는 이벤트가 흐르는 경우와 G04 idle/heartbeat 모두를 통과해야 한다.
제어 turn의 중단만으로는 부족하다. 관측한 0.153.4 결함은 별도로 검토한
런타임 기준으로 대응하며 가짜 이벤트나 숨은 gateway timeout 우회로 감추지 않는다.

전송 후 응답 전에 끊기면 결과를 Unknown으로 기록한다. 최종 이벤트가 없다는
이유만으로 요청을 자동 재실행하지 않는다. 호스트는 재시도 예산·복구 결정을,
gateway는 요청당 한 번의 upstream 시도를 유지한다. 호스트 워크플로에서 도구
결과나 승인이 대기 중일 때는 압축하지 않는다.

[호스트 계약 구현](continuity.md)은 실행 가능한 기록 검증과 합성 conformance를
정의한다. 소비자 영속화와 운영 수락은 별도다.

<a id="g16g17-acceptance-and-migration"></a>

## G16/G17 수락과 마이그레이션

- 도구 결과 재입력, 명시적 압축, 재시작 후 재개와 경로·프로필 불일치 거부를
  재사용 가능한 합성 시험으로 검증한다.
- 필수 sentinel과 완료 도구 결과가 압축·재시작 후 유지돼야 한다. 실제 모델
  요약 품질이나 문학적 품질을 인증하는 시험은 아니다.
- 자격 세대, 바이너리·상태 호환성 또는 경로 설정이 바뀌면 공급자 요청 전에
  재개를 거부해야 한다.
- 명시적 전환은 손실 상태를 기록하고 새 맥락을 사용하며 완료 도구를 실행하거나
  미승인 동작을 상속하지 않아야 한다.
- 프로세스 재시작 전후 취소와 불확실한 결과를 검증한다.
- 소비자 운영 수락과 실제 모델 qualification은 모의 시험·GitHub 검사와 구분한다.

기존 gateway 요청과 설정은 호환된다. 새 비공개 호스트 기록은 gateway 실행의
선택적 기능이며 binding 없는 기존 기록을 검증된 재개 기록으로 자동 승격하지
않는다. 소유자가 검토된 마이그레이션으로 binding을 만들거나 새 맥락을 시작한다.
롤백은 이전에 검증된 실행 파일·설정·이력 조합을 복원한다. 호환되지 않는 새
상태 디렉터리에 과거 코드를 무조건 적용하지 않는다.

기대 효과는 기존 이력 소유자를 활용하고 중복 저장소와 response ID 체계를
피하는 것이다. 비용은 중간 수준의 호스트 통합·복구 시험이며, G05에 필요한
경우에만 작은 범용 gateway 계약 변경을 한다. 요약 품질, 자격 세대의 가용성과
런타임 상태 호환성은 주요 위험이자 명시적 수락 조건이다.

대안인 gateway 상태와 변환 native 압축은 다른 클라이언트를 지원할 수 있지만,
저장 형식, 암호화·키 수명, 인증된 origin binding과 마이그레이션의 승인이
필요하다. 실제 요구가 호스트 소유 경로를 사용할 수 없다는 근거가 생길 때까지 보류한다.

승인은 위 소유권·복구 계약을 수용한다. 소비자 활성화, 상용 권리, 런타임
prerelease 승격, 새 데이터베이스나 생산 배포를 승인하지 않는다.
