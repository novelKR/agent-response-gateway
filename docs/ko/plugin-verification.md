<a id="plugin-verification"></a>

# 플러그인 검증

[English](../plugin-verification.md) | [한국어](plugin-verification.md)

패키지 선언, 실행 파일의 프로토콜 동작, 설치된 게이트웨이 결합, 각 네이티브 플랫폼의 증거를 구분합니다. 선언 검사가 성공해도 handshake를 입증하지 않으며, handshake가 성공해도 managed 영속화나 사용량 소비자 호환성을 입증하지 않습니다. 이 절차는 합성 fixture를 사용하며 실제 외부 공급자를 검증하거나 검토하지 않은 패키지를 승인하지 않습니다.

[작성 계약](plugin-authoring.md), [provider 계약](provider-plugins.md), [보호된 연속성](provider-continuation.md), [사용량 출처](usage-provenance.md)를 함께 읽으십시오. 이 문서는 명령과 결과 해석을 설명하며 특정 산출물이 검사를 통과했다고 주장하지 않습니다.

<a id="prepare-independent-artifacts"></a>

## 독립 산출물 준비

빌드 전에 provider 또는 Recorder 예제 프로젝트 전체를 게이트웨이 체크아웃 밖으로 복사합니다. 라이선스와 소스 파일을 포함하십시오. 독립 프로젝트는 게이트웨이 crate, 참조 엔진, SDK를 가져오지 않습니다. 공개 프로토콜을 충족하면 어떤 구현 언어도 사용할 수 있으며 초기 네이티브 패키지는 선언한 플랫폼에서 실행할 진입점을 제공해야 합니다.

Python 예제에는 빌드 중 선택한 절대 경로에 명시적으로 준비된 Python 3.11+ 인터프리터가 필요합니다. 빌더·설치기·runner는 인터프리터나 패키지를 다운로드하지 않습니다. 대상 선언은 실행 파일을 교차 컴파일하거나 ABI를 입증하지 않습니다. 설치와 실행은 실제 호스트를 다시 확인합니다.

신뢰할 수 있는 경로로 패키지 manifest digest를 확보하고 정확한 manifest, 모든 선언 파일의 해시, 패키지 버전, 역할 프로토콜, 기능 선언을 보존합니다. Digest는 바이트를 식별하며 신뢰하지 않는 입력에서 계산한 값은 제작자를 인증하지 않습니다. 활성화 전에 요청한 권한을 검토하십시오. 환경 변수를 비워도 네이티브 프로세스는 호스트 사용자의 OS 권한을 유지하며 IPC 경계는 샌드박스가 아닙니다.

<a id="standalone-protocol-profiles"></a>

## 독립 프로토콜 프로필

표준 라이브러리만 사용하는 독립 도구의 버전은 `2.0.0`입니다. 도구를 독립적으로 복사하고 검토한 네이티브 코드에만 `--execute`를 선택합니다. 호출 사용자 소유의 비공개 scratch 디렉터리를 사용하십시오. 정적 검사는 패키지를 설치·활성화·실행하지 않습니다.

```sh
python3 -I -B conformance.py --package /absolute/package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256
python3 -I -B conformance.py --package /absolute/provider-package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch --profile synthetic-provider/v1
python3 -I -B conformance.py --package /absolute/recorder-package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch --profile recorder-events/v2 \
  --recorder-state-fixture /absolute/private-disposable-fixture
```

| 프로필 | 검사 | 한계 |
|---|---|---|
| `wire` | Observer Ready/ACK, 일반 provider Ready, 제공한 fixture를 사용하는 Recorder v2 | 일반 provider 의미 검사는 미완료로 남음 |
| `synthetic-provider/v1` | 합성 JSON, SSE, 도구, 숫자 관측과 불투명 상태 메시지 | 임의 공급자의 의미가 아닌 합성 계약을 요구함 |
| `recorder-events/v2` | V1/V2와 잘못된 관측의 ACK, 중복 전달과 재시작 | producer 유지와 동일 ACK를 입증하며 저장된 payload 조회나 전원 장애 내구성을 입증하지 않음 |

선택한 Recorder가 문서화한 초기화 절차로 상태를 준비합니다. Runner는 제공된 fixture를 복사하고 링크와 겹치는 경로를 거절하며 원본을 변경하지 않습니다. 초기화 방법을 추측하거나 운영 저장소를 열지 않습니다. Codec과 Recorder v1 실행 runner는 아직 제공하지 않습니다. 정확한 제한과 프로필 요구사항은 [도구 참조](../../tools/plugin-conformance/README.md)를 확인하십시오.

<a id="interpret-versioned-evidence"></a>

## 버전별 증거 해석

[보고서 스키마](../../schemas/gateway-plugin-conformance-report-v2.schema.json)는 `gateway-plugin-conformance-report/v2`를 정의합니다. 검사 목록과 함께 `tool_version`, `tool_sha256`, `package_sha256`, `fixture_sha256`, 패키지·역할 계약, `target`, `host_target`, `profile`을 보존하십시오. Fixture 식별자가 null이면 fixture digest가 확정되지 않았다는 뜻이며 다른 fixture의 증거가 아닙니다. Report v1 reader는 v2를 명시적으로 지원해야 합니다.

전체 통과를 위해서는 모든 필수 검사가 `pass`여야 합니다. 실패한 검사가 있으면 `fail`, 필수 검사 범위가 미완료이면 `not-run`입니다. 필수가 아닌 `host.integration` 검사는 독립 프로토콜 검사에 성공해도 `not-run`으로 남습니다. 정적 검사만 성공하면 실행 범위의 미완료를 유지하면서 종료 코드 0을 반환하며, 요청한 실행이 미완료이면 2, 검사 실패이면 1을 반환합니다. 고정 진단 코드는 요청·응답 본문, 자격증명, 자식 출력, 로컬 절대 경로를 제외합니다. 보고서는 로컬 증거이며 서명이나 증명서가 아닙니다.

<a id="installed-gateway-acceptance"></a>

## 설치된 게이트웨이 수용 검증

사전 빌드한 정상 게이트웨이와 명시적 관리자·예제 경로로 [설치 수용 harness](../../scripts/provider_acceptance.py)를 실행합니다. Harness는 공개 프로젝트와 도구를 모든 체크아웃 밖의 임시 상태로 복사하고 일반 관리자로 패키지를 빌드·설치하며 명시적 권한과 모델 경로를 선택하고 설치가 게이트웨이 바이너리 digest를 변경하지 않았는지 확인합니다.

```sh
python3 -B scripts/provider_acceptance.py \
  --binary /absolute/build/agent-response-gateway \
  --manager /absolute/tools/extension_manager.py \
  --provider-example /absolute/projects/provider \
  --recorder-example /absolute/projects/recorder \
  --query-recorder /absolute/build/gateway-usage-recorder
```

Harness는 생성한 합성 자격증명, 숫자 loopback HTTP, 프록시 상속·리디렉션 차단, 제한된 자식 정리를 사용합니다. 네이티브 JSON/SSE/도구 교환, 실제 게이트웨이 프로세스 재시작과 managed 상태 재개, 이벤트 바이트 보존과 정확한 플러그인 사용량 정체성을 검사합니다. 추가 검사는 잘못되거나 누락된 숫자 관측, 오래되거나 변경된 연속성 입력 거절, 패키지 교체, 미완료 시도 복구를 포함합니다. 완료 범위는 출력된 검사 목록에서 확인하십시오. 소스 시험이나 복사한 프로젝트만으로 호스트 실행이 입증되지는 않습니다.

별도 `gateway-plugin-acceptance-report/v1` 보고서의 `pass_positive_scope`는 완료된 범위에만 적용됩니다. 남은 검사는 명시적으로 `not-run`이며 이 상태를 전체 검증 완료와 동일시하지 마십시오. `--query-recorder`를 생략하면 비호환 조회 backend 검사는 실행되지 않습니다. 정확한 바이너리·패키지 해시와 플랫폼을 포함한 이 보고서를 독립 보고서, CI 결과, 실제 공급자 증거와 구분하여 보존하십시오.

<a id="platforms-and-offline-containers"></a>

## 플랫폼과 오프라인 컨테이너

지원하는 각 Linux 또는 macOS 대상에서 네이티브 실행 검사를 별도로 수행합니다. Windows 네이티브 플러그인 실행은 미지원입니다. 따라서 Windows의 로컬 full 검증에서는 이 네이티브 수용 검사가 종료 코드 2로 미완료 상태를 유지하며, CI는 해당 플랫폼에서 네이티브 단계를 명시적으로 건너뜁니다. 다른 대상의 정적 수락과 비호환 실행 호스트에서의 거절은 별도 검사입니다. Linux 컨테이너는 macOS 실행 파일이나 인터프리터 경로를 검증하지 않습니다.

[도구 배포 절차](../../tooling/plugin-tools/README.md)로 정확한 소스와 선택한 적합성 스크립트를 묶습니다. 오프라인 빌드 전에 고정된 base 이미지를 검토하고 미리 준비하십시오. 네트워크 차단, 읽기 전용 root, 최소한의 합성 입력 mount, 전용 쓰기 가능 scratch, root가 아닌 숫자 사용자, CPU·메모리·PID 제한을 적용합니다. 운영 저장소, 자격증명, 홈 디렉터리, Docker socket을 전달하지 마십시오. 패키지 바이트는 이 검증 환경 안에서도 신뢰한 네이티브 코드이며 컨테이너는 제품 런타임의 신뢰 모델을 바꾸지 않습니다. 다른 대상의 정적 증거와 네이티브 실행 보고서를 구분하여 보존하십시오.

<a id="recorder-consumers-and-rollback"></a>

## Recorder 소비자와 롤백

독립 Recorder는 자체 저장소로 event/ACK IPC를 구현할 수 있습니다. 이 호환성은 일반 SQL 조회 계약을 구현하지 않습니다. 관리·team reader는 기본 제공 Recorder의 SQLite 구조만 지원하며 비호환 외부 저장소를 설정하면 명시적으로 실패해야 합니다. 독립 fixture의 ledger 검사는 관리/Web 조회 지원을 입증하지 않습니다. 로컬 commit, 원격 내보내기 receipt, 소비자의 해석을 구분하십시오.

교체나 롤백 전에 영향받는 writer를 중지하고 원래 데이터베이스, sidecar, 설정, 정확한 이전 패키지 바이트, 일치하는 연속성 키를 보존합니다. 새 모델 경로를 비활성화한 뒤 호환 바이너리·패키지를 다시 선택하거나 새 세션을 시작하십시오. 패키지 교체는 이전 세션을 자동 이행하지 않습니다. 구버전 바이너리가 새 상태나 V2 사용량을 읽는다고 보장하지 않으므로 문서화된 호환 backup·upgrade 절차를 사용하고 버전 표식을 낮추거나 기록된 출처를 다시 쓰지 마십시오. 미완료 시도는 자동 추론 재시도가 아닌 명시적 호스트 복구가 필요합니다. [연속성 복구](provider-continuation.md)와 [사용량 저장 호환성](usage-provenance.md)을 확인하십시오.
