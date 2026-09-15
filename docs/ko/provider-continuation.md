<a id="protected-provider-continuation"></a>

# 보호된 provider 연속성

[English](../provider-continuation.md) | [한국어](provider-continuation.md)

호스트는 기존 native replay와 함께 불투명 `gateway-provider/v1` 상태를 위한 인증된
버전별 영속화를 구현합니다. **운영 provider 경로 활성화는 아직 사용할 수 없습니다.**
합성 qualification에서 이 구조를 검증할 수 있으며 공개 운영에는 연속성·사용량 출처·Recorder
수용 검증의 결합이 필요합니다. 플러그인의 managed 기능 선언만으로 경로가 활성화되거나
세션이 승인되지는 않습니다.

[Provider wire 계약](provider-plugins.md), [관리형 실행·복구 계약](managed-continuation.md),
[호스트 소유 이력 계약](continuity.md)을 함께 읽으십시오. 플러그인은 공급자 상태를 해석하고,
호스트는 승인, HTTP/auth, 세션 승인, 암호화, 영속화와 공개를 소유합니다.
플러그인에는 저장소 handle, 암호화 키나 호스트 control token을 전달하지 않습니다.

<a id="state-and-exact-binding"></a>

## 상태와 정확한 결합

Managed 요청은 호스트가 승인한 history span, pending-tool 상태와 기존 승인 요청·경로
값만 전달합니다. 각 상태에는 format label, 양수인 부호 없는 32비트 version,
canonical padding을 적용한 표준 base64가 있습니다. 호스트는 decoded 1 MiB 한도를 검사하고
비정규 인코딩을 거절합니다. 전체 직렬화 replay record에는 공개 출력, 결합 metadata와
base64 증가분을 포함한 별도의 2 MiB 한도가 있습니다. 상태 자체가 개별 한도 안에 있어도
전체 record 한도를 초과할 수 있습니다.

첫 durable checkpoint가 format/version을 고정합니다. 이후 checkpoint는 이 쌍과 정확한
플러그인 역할 protocol, provider protocol, package ID/version, package digest, executable
digest를 유지해야 합니다. Record는 session origin을 통해 확정된 provider·model·route를
결합하며 자격증명 소유 realm/generation, session, epoch, parent response, 입력 길이와 입력
digest도 결합합니다. 플러그인은 호스트가 선택한 이 결합을 응답의 식별자로 바꿀 수 없습니다.

Managed completed·awaiting-tools 결과에는 유효한 불투명 상태가 필요합니다. Stateless
결과에는 명시적 상태 부재가 필요합니다. Incomplete managed 결과는 재개 가능한 checkpoint가
아닙니다. 도구 출력은 승인된 도구 계약과 알려진 pending call에 일치해야 합니다.
게이트웨이는 도구를 실행하거나 해당 동작을 승인하지 않습니다.

Managed 응답에서 summary가 빈 reasoning 항목은 최대 하나만 허용하며,
해당 항목은 호스트 envelope를 받는 첫 번째 reasoning 항목이어야 합니다.
호스트는 이 조건을 벗어난 빈 summary 배치를 checkpoint 커밋 전에 거절하며,
JSON과 SSE에 같은 규칙을 적용합니다. 빈 summary 항목이 없는 응답은 모든 공개
reasoning summary를 보존하고 기존 carrier 처리 방식으로 envelope를 받습니다.

<a id="replay-versions-and-durable-barriers"></a>

## Replay 버전과 durable 장벽

새 provider record는 `gateway-continuation/v3`와 `arg-continuation-v3.` envelope prefix를
사용합니다. 버전 prefix와 key ID는 인증 데이터에 참여하므로 prefix를 바꿔 암호문을 다른
버전으로 해석할 수 없습니다. 기존 V1/V2 record의 원래 바이트, envelope 규칙과 finalized
digest는 유지됩니다. 내부 표현으로 바꾸기 전에 인증하고 authoritative 저장 digest와
비교합니다. 행·암호문의 일괄 migration은 없으며 continuation SQLite 테이블은 그대로입니다.

명시적으로 구성한 managed provider 경로가 있으면 manifest 버전은
`gateway-embedded-manifest/v10`, readiness는 `gateway-ready/v10`, 확장 manifest/readiness는
`gateway-extended-manifest/v10` / `gateway-extended-ready/v10`입니다. Replay 선언은 다음과 같습니다.

```json
{"read":[1,2,3],"write_builtin":2,"write_provider":3}
```

내장 공급자만 사용하는 managed 구성은 기존 read/write 선언을 유지합니다. 호스트는 선언된
전체 스키마와 정확한 configuration·execution digest를 이해해야 합니다. 미지원 버전은
오류이며 암묵적 하향은 없습니다. 버전 선언이 운영 provider 활성화 제한을 해제하지 않습니다.

기존 attempt 예약과 SQLite finalization 장벽을 재사용합니다. 호스트는 finalize 전에 현재
session revision, origin, epoch와 parent를 검사합니다. 인증된 parent는 같은 상태 결합의
호환 V3 record여야 합니다. 공개 텍스트·추론 진행은 durable finalization 전에 나올 수 있지만,
실행 가능한 도구 완료, 복구 envelope와 성공 terminal 출력은 기다려야 합니다. 저장이나
공개 전 검사 실패는 조정이 필요한 미완료 attempt를 남기며 자동 inference 재시도나 상태
생성을 승인하지 않습니다.

<a id="restore-repair-and-rollback"></a>

## 복원·복구·롤백

재시작에는 같은 데이터베이스, 안정적인 key/key ID, 정확한 패키지와 호환되는 host origin·이력이
필요합니다. Restore는 전달받은 session snapshot을 현재 authoritative session과 비교하며,
revision, head, status, pending tool과 portable-history 결합을 포함합니다. 오래된 snapshot,
잘못된 origin, 패키지 변경, 상태 format/version 변경, 손상 payload와 누락된 실행 record는
명시적으로 실패합니다.

저장 payload가 없는 finalized record는 인증, 정확한 origin/session 검사와 원래 finalized
digest 일치를 확인한 뒤에만 전달된 envelope로 복구할 수 있습니다. Authoritative payload가
있으면 인증하고 비교하며 암묵적으로 덮어쓰지 않습니다. Pending·unknown attempt를 완료
상태로 복구할 수 없으며 기존의 명시적인 recovery·epoch 전이 절차를 유지합니다.

패키지 교체는 이전 세션을 이행하지 않습니다. 정확한 이전 패키지를 복원하거나 새 세션을
시작하십시오. 롤백 전에 원래 데이터베이스, sidecar, 일치하는 키, 호스트 결합과 이력을
보존하십시오. 새 경로를 비활성화하고 호환 바이너리·패키지를 선택하거나 호환 backup을
복원합니다. 구버전 바이너리의 V3 읽기를 보장하지 않으므로 새 record를 V2로 다시 쓰거나
버전 marker를 낮추면 안 됩니다. 자동 상태 변환·만료·재시도는 없습니다.

<a id="rust-source-compatibility"></a>

## Rust 소스 호환성

`ReplayRecord`는 이제 `V1`, `V2`, `V3`를 포함하므로 enum을 match하는 코드는 새 variant를
명시적으로 처리해야 합니다. 항상 `ReplayV2`를 만들던 기존 공개
`ReplayRecord::normalize()` 메서드는 제거됩니다. `NormalizedReplay`는 crate 내부
`into_normalized()`를 통해 사용하는 직렬화 불가능한 내부 표현이며 공개 저장 형식이나
플러그인 wire 형식의 대안이 아닙니다.

Rust 호출자는 `Protector::open_record` 또는 `Runtime::restore_record`가 반환한 원래
`ReplayRecord`를 유지하고 필요한 인증·세션 검사를 거친 뒤 버전별 필드를 사용해야 합니다.
`ReplayRecord::validate()`는 구조·결합 일관성을 검사하며 그 자체로 세션을 승인하지 않습니다.
Digest/envelope 처리에는 원래 버전의 직렬화를 유지하십시오. 공개 플러그인 작성자는 계속
wire 메시지를 사용하며 gateway crate가 필요하지 않습니다.

내부 native history는 provider 바이트를 builtin variant에 넣지 않고 내장 replay와 provider
상태를 구분합니다. `VerifiedProviderHistory`는 더 이상 외부 소스 호출자에게 변경 가능한
history segment를 공개하지 않습니다. 승인된 history는 호스트 경계에서 구성하고 선택한
역할에 맞게 명시적으로 변환합니다. 이 소스 변경은 기존 codec V1/V2 wire 의미를 바꾸지 않습니다.
