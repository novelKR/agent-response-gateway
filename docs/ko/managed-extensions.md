<a id="audited-local-extension-management"></a>
<a id="concurrency-and-evidence"></a>
<a id="installed-selected-and-effective-state"></a>
<a id="store-and-source-ownership"></a>
<a id="verification-and-recovery"></a>

# 로컬 확장 관리의 영속 감사

[English](../managed-extensions.md) | [한국어](managed-extensions.md)

선택형 `gateway-management-extensions` 라이브러리는 기존 native extension과 profile-pack
관리자를 영속 관리 journal에 연결합니다. Gateway·listener·상주 worker를 시작하지 않으며,
HTTP·대시보드 조립은 별도입니다. 기본 Gateway는 이 패키지에 의존하지 않습니다.

## 저장소와 원본의 소유권

신뢰된 호스트는 대상 하나, 기존 패키지 저장소, 비공개 증거 디렉터리, 패키지 SHA-256으로 고정한
이름 있는 로컬 원본을 등록합니다. Native 관리에는 절대 경로의 Python interpreter와 관리자
script 및 두 파일의 hash도 등록합니다. Native 관리에는 Python 3.11+와 Linux/macOS가
필요하며 profile pack은 기존 이식 가능한 Rust 구현을 사용합니다. 미지원 native 플랫폼은
adapter 상태를 만들기 전에 거절합니다.

`Manager::initialize`는 `gateway-extension-manager/v1` marker를 새로 만들며 덮어쓰기를
거절합니다. `Manager::open`은 해당 스키마를 요구합니다. 증거 디렉터리는 단일 소유자 lease를
사용하고 Windows 비공개 접근은 호스트 ACL 책임입니다. 호스트는 기존 관리자나 문서화된 저장소
구조에 따라 패키지 저장소를 명시적으로 준비합니다. 설치 원본을 다운로드하거나 자동 발견하지
않습니다. HTTP 호출자는 interpreter·shell·파일시스템 경로 대신 등록된 source ID를 사용해야 합니다.

상태와 원본 검사는 신뢰된 호스트의 조회 메서드이며 호스트가 조회 권한을 검사해야 합니다.
변경은 `Manager::bind`와 관리 journal을 거쳐 최신 actor 권한·명령 digest·예상 저장소
snapshot을 확인합니다. adapter는 provider·팀·관리 credential 값을 읽지 않습니다.

## 설치·선택·실제 적용 상태

Inventory는 설치된 정확한 버전, 검증 결과, 기존 활성화 snapshot을 구분해 보고합니다.
손상된 패키지도 검증 실패로 표시하며, 중단되었거나 알 수 없는 설치 산출물은 숨기지 않고
inventory를 명시적으로 실패시킵니다. 검사는 설치 패키지 128개와 디렉터리 항목 1024개로
제한합니다. Profile inventory에는 패키지 고지 원문이나 모델 payload를 복사하지 않습니다.

신뢰된 runtime 소유자는 instance·관측 시각·구성/실행 digest·정확한 패키지 선택을 담은
`EffectiveSelection`을 제공할 수 있습니다. 해당 관측이 없으면 실제 적용 상태는 알 수 없습니다.
이 항목은 실행 구성에 포함됨을 의미하며 codec 프로세스의 상주 실행을 뜻하지 않습니다. Codec v2는
계속 요청별로 실행됩니다. 선택 변경은 제공된 runtime 관측을 갱신하거나 재시작을 주장하지 않습니다.

| 작업 | Native extension | Profile pack |
|---|---|---|
| 설치 | 정확한 로컬 manifest와 모든 파일 digest 검사 | 정확한 canonical 단일 파일 digest 검사 |
| 활성화 | 요청된 권한 전체와 필요한 등록 recorder binding 요구 | 실행 권한 없음; 정확한 설치 선택 요구 |
| 버전 선택 | 새 권한 부여로 명시적 version/digest 선택; 이전 버전 보존 | 이전 binding을 비활성화한 뒤 정확한 대체 버전 활성화 |
| 비활성화 | 다음 시작 activation binding을 제거하고 패키지/상태/사용량 바이트 보존 | 활성화만 제거하고 모든 패키지 바이트 보존 |
| 패키지·데이터 제거 | 미지원 | 미지원 |

관리형 native 활성화와 버전 선택은 별도 작업 권한을 사용합니다. 활성화로 기존 활성 버전을
교체할 수 없으며 버전 선택에는 기존 binding이 필요합니다. 기존 CLI의 명시적인
활성화/교체 동작은 유지합니다.

`removal_supported`는 false입니다. 미지원 작업은 성공 대신 실패합니다. 이 adapter에는
uninstall·데이터 삭제·원격 검색/다운로드·업데이트 서비스·Gateway 재시작·모델 재시도가 없습니다.
설치·활성화는 패키지 코드를 실행하지 않습니다. Recorder를 observer로 교체하면 사용량 데이터를
보존하면서 recorder activation binding만 해제합니다.

## 동시 변경과 증거

Native guarded 작업은 기존 Python 변경 잠금 안에서 generation과 inventory digest를
검사합니다. 설치를 포함한 profile 작업은 기존 Rust writer marker를 사용하고 같은 사전 조건을
그 안에서 확인합니다. 설치는 활성화 generation을 바꾸지 않고 inventory를 변경할 수 있으며
digest가 그 차이를 감지합니다. 설치 변경 직전에 정확한 원본 바이트도 다시 검사합니다.
기존 CLI 변경 작업도 같은 잠금을 사용합니다.

각 guarded 관리자는 변경 잠금을 해제하기 전에 결과와 변경 후 inventory를 함께 확보합니다.
adapter는 작업 ID·요청 digest·저장소 digest·변경 후 상태·결과 digest를 불변 증거로 기록합니다.
변경 전후 canonical inventory는 별도 snapshot 디렉터리의 비공개 구성 산출물로 보존하며
완료 증거에는 해당 digest만 포함합니다.
작업 증거에는 패키지/구성 원문·고지·credential·모델 내용이 들어가지 않습니다. 증거 파일·패키지
파일시스템 변경·감사 SQLite는 단일 원자적 트랜잭션이 아닙니다. 결과 기록 누락이나 불완전한
증거는 미확정으로 남으며 상태 조정은 완전한 증거만 읽고 패키지 작업을 반복하지 않습니다.

`external_change`는 미확정 관리 변경을 포함해 마지막 확인된 adapter inventory와의 차이를
보고하며 행위자를 지정하지 않습니다. 새로 검토한 작업은 최신 snapshot으로 제출할 수 있고,
오래된 snapshot은 거절합니다. 재시작한 adapter는 새로운 controller epoch를 사용하므로
이전 사전 검증 상태를 재사용할 수 없습니다.

Native helper의 출력과 실행 시간은 제한되며 상위 환경을 상속하지 않습니다. 확인할 수 없는
helper 결과는 미확정입니다. 등록된 원본 파일·Python·관리 script를 다른 호스트 관리자의
변경으로부터 보호해야 합니다. Unix native 잠금은 프로세스 종료 시 해제되며, 중단된 profile
writer는 명시적인 검사를 위해 기존 복구 marker를 남깁니다. marker 제거·활성화 복구·증거 삭제·
미확정 작업 재실행을 자동으로 수행하면 안 됩니다.

## 검증과 복구

```sh
cargo test -p gateway-management-extensions --locked
cargo test -p agent-response-gateway --test profile_packs --locked
python3 -B -m unittest discover -s scripts/tests -p test_extension_manager.py -v
```

Native Rust fixture는 기본적으로 `python3`를 찾고 Python 3.11+를 요구합니다.
필요하면 `MANAGEMENT_TEST_PYTHON`으로 지원되는 interpreter를 명시합니다.
이 테스트 설정은 제품 driver를 구성하지 않습니다.

합성 테스트는 정확한 설치·낡은 generation과 inventory digest·손상·새 권한 부여·이전 버전/상태
보존·외부 CLI 변경·감사 시작/결과 기록 실패·완료 증거 조정을 다룹니다. Native fixture는
codec v2를 포함한 실행되지 않는 패키지 바이트를 사용하며 패키지 코드나 live provider를 호출하지
않습니다. Runtime/library 호스트와 최종 배포물은 별도 수용 검사가 필요합니다. 실행 파일을 되돌릴
때는 호환되는 패키지 저장소·비공개 adapter 증거·별도 감사 journal을 보존합니다. 이전 패키지
선택은 데이터 형식을 역변환하거나 완료된 모델 작업을 재실행하지 않습니다.
