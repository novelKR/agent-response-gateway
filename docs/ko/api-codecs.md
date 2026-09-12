<a id="change-and-recovery"></a>
<a id="external-api-codecs"></a>
<a id="ipc-lifecycle-and-validation"></a>
<a id="package-and-route-selection"></a>
<a id="responsibilities-and-trust"></a>

# 외부 API codec

[English](../api-codecs.md) | [한국어](api-codecs.md)

검토한 네이티브 패키지를 선택해 버전이 명시된 IPC로 모델 API 전체를 변환한다.
참조 패키지는 내장 경로와 같은 codec 라이브러리로 Responses, Messages,
Chat Completions, Gemini Interactions를 지원한다. 요청 승인, HTTP 전송,
자격 증명, 최종 검증, 사용량 기록과 durable 연속 대화 저장은 호스트 게이트웨이가 소유한다.

## 책임과 신뢰

Codec은 승인된 Responses 요청, 유효 기능 선언과 인증된 native replay 구간을
받는다. 공급자 JSON 요청, 복원한 Responses 출력 또는 공개 진행 이벤트와
버전이 명시된 native replay를 반환한다. IPC로 공급자 URL, 인증 헤더,
환경 자격 증명, 세션 토큰, 암호화 키, 데이터베이스 핸들이나 호스트 저장 경로를
받지 않는다. 참조 codec은 HTTP 요청, 도구 실행이나 데이터베이스 쓰기를 수행하지
않는다. 이러한 작업의 콜백, 임의 변환 스크립트 API, 재시도, fallback, 암묵적
재시작, 다운로드나 hot reload도 없다.

네이티브 패키지는 신뢰하는 코드이며 OS 샌드박스가 아니다. 환경을 비우고 프로토콜을
제한해도 악의적인 동일 사용자 네이티브 코드가 자체 OS 권한을 사용하는 것을 막지
못한다. `read_model_payload`와 `transform_model_protocol` 권한을 부여하기 전에
실행 파일과 출처를 검토한다. 본문 접근은 HTTP 메타데이터 observer보다 훨씬 민감하다.
패키지는 지원하는 Linux 또는 macOS 대상과 일치해야 한다. Windows에서는 네이티브
codec 활성화를 거절하며 일반 게이트웨이와 비실행 프로파일 팩은 계속 사용할 수 있다.

코어는 요청별 전용 프로세스를 시작하기 직전에 실행 파일 digest를 다시 확인한다.
입출력은 소켓으로 연결한 표준 스트림을 사용하며 stderr는 버리고 상속 환경을 비운다.
패키지별 전용 디렉터리는 작업 디렉터리일 뿐 codec 데이터베이스 계약이 아니다.
패키지 파일과 디렉터리는 기존 네이티브 확장의 소유자·권한 검사를 따른다.
같은 소유자가 동시에 악의적으로 파일을 변경하는 경우는 이 신뢰 모델 밖이다.

## 패키지와 경로 선택

정확히 검토한 게이트웨이 소스에서 참조 실행 파일을 빌드한다.

```sh
cargo build --locked
cargo build --locked --example api_codec
python3 -B scripts/extension_manager.py package \
  --binary /absolute/path/target/debug/examples/api_codec \
  --license-file /absolute/path/LICENSE --output /absolute/path/codec-package \
  --id reference-codec --version 1.0.0 --role api_codec
```

출력된 정확한 패키지 digest와 두 codec 권한으로 기존 오프라인
[설치·활성화 절차](extensions.md)를 사용한다. 패키지는
`gateway-extension-package/v1`, 프로토콜 `gateway-api-codec/v1`, 상태 계약
`request-memory/v1`을 사용한다. 설치 후에는 비활성이며 활성화만으로 codec에
트래픽이 연결되지 않는다. 기존 패키지 크기·고지·플랫폼·불변 설치 제한이 적용된다.
예제 명령은 로컬 시험을 위해 프로젝트 라이선스를 포함한다. 재배포할 때는
[릴리스 절차](release.md)에 따라 정확한 바이너리의 대응 소스와 의존성 고지도 필요하다.

```sh
python3 -B scripts/extension_manager.py install \
  --store /absolute/path/store --package /absolute/path/codec-package \
  --expected-sha256 <exact-package-sha256>
python3 -B scripts/extension_manager.py enable \
  --store /absolute/path/store --id reference-codec --version 1.0.0 \
  --package-sha256 <exact-package-sha256> \
  --grant read_model_payload --grant transform_model_protocol
```

대상 모델마다 `api_codec = "reference-codec"`를 지정하고 `check-config`,
`manifest`, `serve`에 `--extensions-lock /absolute/path/store/active.json`을
전달한다. 모델은 기능 프로파일과 인증도 명시해야 한다. Responses 경로는 요청
승인을 검사할 수 있도록 도구 호환 정책을 선택해야 한다. `api_codec`가 없는
모델은 내장 경로를 유지하며 해석되지 않은 codec 선택은 오류다. 프로파일 팩
import와 codec 선택을 함께 사용할 수 있다. 외부 codec은 지원하는 API 계약을
구현하며 API enum 값이나 미지원 기능 권한을 추가하지 않는다.

오프라인 검사는 codec을 실행하지 않고 바이트를 검증한다. 선택된 경로는
`gateway-embedded-manifest/v6`와 `gateway-ready/v6`, 확장 wrapper는
`gateway-extended-manifest/v6`와 `gateway-extended-ready/v6`를 사용한다.
활성 codec을 사용하지 않는 경우에도 wrapper는 v6다. 선택한 각 경로에는
패키지 식별, 실행 파일 해시, 프로토콜, replay 버전과 권한이 포함된다.
패키지 식별은 경로 어댑터 식별에도 참여한다. 호스트는 스키마를 이해하고 검사와
준비 응답의 digest를 비교한 뒤 에이전트를 시작해야 한다.

## IPC 수명 주기와 검증

각 프레임은 부호 없는 4바이트 big-endian 길이와 UTF-8 JSON으로 구성된다.
프레임은 128 MiB로 제한되며 요청·응답과 이벤트 누적에는 게이트웨이 설정 한도도
적용된다. 중복 키, 알 수 없는 필드와 미지원 버전은 실패한다. 응답은 `protocol`과
정확한 요청 `sequence`를 되돌려준다. 최초 준비 응답은 sequence 0으로 네 API와
native replay 버전 1을 선언한다. 이후 sequence는 1씩 증가한다.

[타입 계약](../../src/codecs/contract.rs)은 `Request.operation`과 `Reply.value`를
정의한다. 첫 작업은 `prepare`이며 다음 작업으로 `json` 또는 `stream`을 선택한다.
스트리밍에서는 파싱한 공급자 SSE `event`를 하나씩 보내고 `finish`로 끝낸다.
게이트웨이가 SSE 바이트 프레이밍과 HTTP 취소를 소유한다. 참조 엔진에는 Rust ABI
경계가 없으며 외부 구현은 내부 Rust 구조체나 메모리 배치 대신 버전이 명시된
JSON 계약을 사용한다.

시작과 각 IPC 교환에는 부분 읽기·쓰기를 포함한 총 3초의 제한이 있다.
실패한 요청은 더 처리하지 않는다. 같은 시도를 재시도하거나 대체 프로세스를 띄우지
않는다. 세션이 해제되면 취소나 공급자 전송 중단을 포함해 직접 자식 프로세스를
종료하고 회수한다. 네이티브 신뢰 계약은 daemon화를 금지한다. 자손 프로세스를
격리하는 기능은 아니다. 요청별 프로세스 실행에는 시작과 digest 검사 비용이 추가된다.

코어는 복원된 도구 식별·선택·인자 JSON·등록 문법 출력과 완결된 Responses
이벤트 수명 주기를 독립적으로 검증한다. 실행 가능한 인자는 최종 검증과 설정된
기록 장벽 뒤에서 공개된다. 항목 순서를 보존하기 위해 stateless 도구와 뒤따르는
출력 항목은 검증할 때까지 보관한 뒤 원래 출력 인덱스 순서로 공개한다. 도구보다
앞선 텍스트는 바로 진행할 수 있다. Managed 진행은 공개 텍스트와 reasoning만
허용하며 표시한 텍스트는 최종 출력과 일치해야 한다. Native replay는 공개
Responses 출력과 분리하며 클라이언트 소유 상태 핸들로 내보내지 않는다.

사용량은 코어가 관측한 실제 공급자 바이트에서 가져온다. Codec 사용량은 이 관측과
대조하며 코어가 숫자의 출처와 허용된 공급자 식별을 유지한다. Codec은 recorder
성공이나 durable 연속 대화 토큰을 만들 수 없다. 연속 대화의 승인·보호·최종화·
복구와 기존 replay 형식의 권한은 코어에 남는다.

## 변경과 복구

선택한 패키지를 비활성화하고 검토한 정확한 대체 버전을 활성화한 뒤 manifest를
검사하고 게이트웨이를 재시작한다. 패키지 식별이 바뀌면 이전 경로 origin이
유효하지 않게 된다. 다른 코드로 이전 세션을 암묵적으로 재개하지 않는다.
롤백은 보관된 이전 바이트를 명시적으로 선택한다. 기존 저장소와 replay 복구
절차가 계속 적용되며 codec 소유 migration이나 두 번째 데이터베이스는 없다.
잘못된 codec 응답이나 프로세스 실패로 코어가 소유한 시도가 남으면 일반적인
호스트 조정 절차가 필요할 수 있다.

시험은 공유 참조 구현과 독립적인 프레이밍·순서·크기·EOF·기한·공개 출력·
프로세스 회수 검사를 결합한다. 실제 Codex 시나리오는 합성 공급자로 내장·외부
경로, 도구 순서, 문법 실패, 취소, managed replay와 사용량 recorder 장벽을
검증한다. 이러한 검사는 실제 공급자를 검증하거나 제3자 codec의 네이티브 권한을
승인하지 않는다.
