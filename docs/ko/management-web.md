<a id="browser-session-boundary"></a>
<a id="build-and-source-provenance"></a>
<a id="read-only-management-web"></a>
<a id="synthetic-browser-verification"></a>
<a id="views-and-observation-semantics"></a>

# 조회 전용 관리 Web

[English](../management-web.md) | [한국어](management-web.md)

`management-web/`은 [관리 HTTP API](management-api.md)를 조회하는 선택형 독립
Vue 애플리케이션입니다. 실행·구성 정보, 확장 목록, 사용량, 영속 관리 작업을 표시합니다.
Gateway 프로세스를 만들지 않으며 기본 Gateway의 의존성을 늘리지 않습니다. 신뢰된
호스트가 검증된 정적 파일과 관리 API를 같은 numeric loopback Origin에서 제공해야 합니다.
실제 실행·확장 adapter 조립은 별도로 검증합니다. 합성 브라우저 fixture는 제품 구성이 아닙니다.

제공 [Standalone 관리 애플리케이션](standalone-management.md)은 명시적 저장소, 조회 Web, 선택형 Team 접근으로 이 adapter를 조립합니다.

## 화면과 관측 의미

애플리케이션은 `gateway-management-http/v1`과 `gateway-management-state/v1`을
사용합니다. 인증된 주체에게 허용된 조회 작업에 따라 탐색 메뉴를 표시합니다. 실행 module은
`gateway-runtime-status/v1`, 확장 module은 `gateway-extension-status/v1`으로
조회합니다. 다른 module은 개요에 계약과 관측 상태를 유지합니다. 미지원·미관측 module을
성공적으로 조회한 빈 목록으로 바꾸지 않습니다.

API 응답 시각과 module 관측 시각을 따로 표시합니다. 새로고침 실패 시 이전 관측과 경고를
유지합니다. 다음 실행에 선택한 구성과 실제 실행 구성을 구분합니다. 확장 행에는 설치 버전,
다음 시작 선택, 실제 실행 구성의 선택과 요청·부여된 권한을 표시합니다. 실제 적용을 관측하지
못했다면 알 수 없는 상태입니다. Codec 포함 여부는 상시 프로세스 실행을 의미하지 않습니다.
설치·제거·활성화·구성 변경·프로세스 제어·상태 조정 버튼은 제공하지 않습니다.

사용량은 null 카운터, 부분·미관측 결과와 미완결 시도를 보존합니다. JavaScript의 안전한
정수 범위를 넘는 카운터는 정확한 숫자 문자열로 유지합니다. JSON reviver의 원본 숫자를
제공하지 않는 브라우저는 해당 응답을 명시적으로 거절합니다. 금액을 계산하거나 HTTP
응답에서 모델 완료를 추정하지 않습니다. 작업 이력은 journal cursor의 오름차순으로
20개씩 읽습니다. 상세 화면은 기록 상태와 현재 관측 상태를 함께 보여 주며 과거 이벤트를
덮어쓰지 않고 미확정을 표시합니다.

첫 작업 페이지가 선택된 활성 세션에서 화면이 보일 때 15초마다 조회합니다. 이전 이력을
추가로 읽으면 자동 조회를 멈추고 수동 새로고침은 첫 페이지로 돌아갑니다. 각 fetch에는
15초 제한과 2 MiB 응답 제한이 있습니다. 변경 작업 자동 제출·재시도는 없습니다.

## 브라우저 세션 경계

전용 조회 credential만 대시보드를 열 수 있습니다. 제출한 입력은 지우며 credential과
응답 데이터를 브라우저 저장소에 보존하지 않습니다. 언어 설정만 로컬에 저장합니다.
관리 서버가 경로 범위의 HttpOnly·SameSite=Strict cookie를 제공하고 권한을 다시
검증합니다. 만료·폐기된 세션은 인증 화면으로 돌아갑니다. 로그아웃은 현재 조회 세션을
제거합니다. 변경용 관리 credential로는 브라우저 세션을 만들 수 없습니다.

클라이언트는 상대 경로의 같은 Origin에 요청하고 redirect를 거절합니다. 원격 API
fallback이나 내장 mock은 없습니다. 호스트는 정확한 Host·Origin, 제한적인
Content-Security-Policy, wildcard 없는 CORS, `no-store`·`nosniff`·`no-referrer`
헤더를 fixture처럼 적용해야 합니다. 정적 파일은 해당 loopback Origin에서 공개될 수 있지만
모든 관리 데이터에는 인증이 필요합니다. 정적 페이지 제공은 데이터 조회 권한이 아닙니다.

## 빌드와 소스 근거

독립 빌드에는 Node 24.21.0과 npm 11.19.0을 사용합니다.

```sh
npm ci --prefix management-web --ignore-scripts
npm test --prefix management-web
npm run build --prefix management-web
node management-web/scripts/check-output.mjs
```

커밋된 lock은 Vue 3.5.42, Vite 5.4.21, Vue plugin 5.2.4를 재사용합니다.
생성된 `.local/management-web/dist/` 제공에는 Node가 필요하지 않습니다.
문서 사이트는 별도 애플리케이션으로 유지합니다. 고지 목록 plugin은 빌드에서만 공유합니다.
어느 쪽도 Vite 개발 서버를 실행하지 않으며 기존
[개발 서버 제약](documentation.md#web-notices-and-development-server-constraints)을 유지합니다.

Web manifest는 HTTP·상태 계약 버전, 소스 커밋, dirty tree 여부, 조회 전용 범위와
파일별 SHA-256을 기록합니다. 변경하지 않은 프로젝트 라이선스, 정확한 배포 패키지 목록,
원본 고지를 함께 제공합니다. `node management-web/scripts/check-output.mjs COMMIT`은
해당 커밋과 clean 소스 근거를 요구합니다. dirty 로컬 미리보기는 정확한 소스 배포물이나
attestation이 아닙니다. 변경된 라이선스 조건·원본·내장 자산을 검토한 후
`npm run build --prefix management-web -- --record-notices`로 목록을 기록합니다.
해시 기록은 상업적 권한 승인이 아닙니다. 일반 빌드와 CI는 검토를 기록하지 않습니다.

`npm run preview --prefix management-web`은 검증된 정적 파일만
`http://127.0.0.1:43141/dashboard/`에서 제공하며 관리 API는 없습니다. Web 복구는
이전 검증 정적 산출물을 사용합니다. 감사·사용량·continuation 저장소를 변환하거나
삭제하지 않습니다.

## 합성 브라우저 검증

테스트 전용 Rust `web_fixture` example은 실제 관리 router, 조회 세션 인증,
임시 SQLite journal, 합성 module 관측을 빌드된 파일과 함께 제공합니다. Gateway나
provider 호출은 시작하지 않습니다. stdin이 닫히면 종료합니다. 고정 credential은
fixture 소스에만 존재합니다.

```sh
cargo build -p gateway-management-api --example web_fixture --locked
WEB_FIXTURE_BIN="$PWD/target/debug/examples/web_fixture" npm test --prefix management-web
cargo run -p gateway-management-api --example web_fixture --locked -- --assets .local/management-web/dist
```

대화형 fixture에서는 출력된 numeric loopback URL을 열고 대상 `gateway`,
credential `synthetic-browser-read-key-01234567890123456789`를 사용합니다.
이 값들은 제품 credential이 아닌 합성 테스트 값입니다. 두 언어, 키보드 탐색과 dialog
focus, 작은 화면, 서로 다른 확장 선택, 알 수 없는 사용량 카운터, 권한 거절과 로그아웃을
확인합니다. Fixture 통과는 실제 adapter 조립·hosted CI·소비자 운영 승인과 구분합니다.

<a id="appearance-and-shared-presentation"></a>

## 화면 표현과 공통 테마

대시보드는 Light, Dark, System 테마를 제공합니다. 저장한 선택이 없으면 Light를
사용합니다. System을 선택한 동안 브라우저의 색상 설정을 따릅니다.
언어와 테마 선택만 로컬에 저장하며 credential과 서버 상태는 브라우저 저장소에
보존하지 않습니다. 브라우저 저장소가 제한되어도 현재 페이지의 테마를 선택할 수 있습니다.

두 테마는 표면·텍스트·상태·포커스·간격·타이포그래피의 의미 기반 CSS 토큰을 공유합니다.
어느 테마에서도 조회 화면과 권한 규칙은 같습니다. Client와 시각 주입은 내부 화면 표현
접점이며 제품 진입점의 기본값은 항상 실제 same-origin API client입니다.
화면을 해제하면 진행 중인 client 요청과 화면 표현 listener도 정리합니다.


<a id="development-scenario-canvas"></a>

## 개발용 시나리오 canvas

개발 전용 DevDemo는 제품 대시보드의 컴포넌트·API client 파서·테마 토큰을 공유합니다.
Gateway 실행 프로그램이나 공개 배포 구성이 아닙니다.

```sh
npm ci --prefix management-web --ignore-scripts
npm run devdemo --prefix management-web
```

Node 24.21.0과 npm 11.19.0을 사용합니다. 출력된 numeric loopback URL을 엽니다.
기본 포트는 43142이며 `-- --port 43144`로 다른 사용 가능한 포트를 선택할 수 있습니다.
합성 모드에는 Rust·Gateway 구성·credential이 필요하지 않습니다. Ctrl+C로 launcher를
종료합니다. 이 진입점만 개발 HMR을 허용하며 제품·문서 미리보기는 검증된 정적 파일을
계속 제공합니다.

별도 제어판에서 시나리오·화면·테마·언어·viewport·초기화를 선택합니다. iframe은 실제
360·768·1280 CSS pixel 또는 가용 폭의 viewport를 갖습니다. 표는 화면 내부에서
스크롤합니다. 제어판은 합성 데이터임을 항상 표시하고 선택한 호스트의 권한 밖에 있는
화면 선택을 비활성화합니다.

정상·중지 실행, 구성 적용 대기, 외부 변경, 설치·선택·실행 버전 차이, 미관측·미지원 모듈,
팀 본인 사용량, 제한된 Embedded 화면, 빈 목록·긴 목록·페이지 나눔, 로딩, 권한 거절·세션
만료, 연결 오류, 정확한 큰 수·0·미관측 사용량과 성공·실패·미확정 감사 행을 재현합니다.
응답은 고정된 시각과 seed를 사용합니다. 로딩도 실제 client의 15초 제한을 유지하며
초기화로 다시 시작합니다. 초기화·시나리오 변경은 이전 화면을 해제하고 요청을 취소합니다.
화면 설정·시나리오 상태를 저장하지 않으며 실제 모델을 호출하지 않습니다.

공유 의미 기반 CSS 토큰이나 컴포넌트를 수정하면 HMR로 반영됩니다. 개발 서버는
Host·Origin·WebSocket 연결과 파일 제공 범위를 필요한 Web 소스·의존성 디렉터리로
제한합니다. 비공개 상태·임의 workspace 파일을 제공하지 않으며 합성 모드는 관리 API를
제공하지 않습니다.

제품 빌드는 개발 모듈과 fixture·HMR 콘텐츠를 거절합니다. 제품 Web archive에는
DevDemo가 없으며 대응 소스 archive에는 개발 소스가 유지됩니다. 기존 직접 실제 API
fixture 검증은 별도 테스트로 유지합니다. 합성 화면의 성공은 인증·실행 운영 검증을
의미하지 않습니다.


<a id="actual-api-fixture-development-mode"></a>

## 실제 API fixture 개발 모드

같은 UI를 실제 관리 router에 연결해 확인하려면 선택형 fixture launcher를 사용합니다.

```sh
npm run devdemo:fixture --prefix management-web
npm run devdemo:fixture --prefix management-web -- --preset usage
```

이 모드에는 Rust 1.98.0도 필요합니다. Launcher는 잠긴 `web_fixture` example을
저장소의 `target/`에 빌드하고 API 전용 모드로 시작하며 stdin 수명을 소유합니다.
기본 `all` preset은 상태·사용량·감사 조회를 허용하고 `usage`는 사용량 조회만 허용합니다.
둘 다 DevDemo 제어판에 표시하는 기존 합성 조회 key와 정상 API 로그인·로그아웃을
사용합니다. 이 테스트 key만 사용하세요. 실행 fixture에는 최소 플랫폼 환경만 전달하며
provider·관리 credential을 전달하지 않습니다.

제어판은 실제 API 전송과 합성 fixture 상태를 구분해 표시합니다. 시나리오·오류 주입
제어는 비활성화되며 preset은 실행 명령에서 선택합니다. 화면·테마·언어·viewport·초기화는
계속 사용할 수 있습니다. Shell과 canvas는 테마 토큰을 공유하며 화면 설정은 임시로만
유지합니다. 초기화는 화면을 다시 만들 뿐 API 인증을 우회하거나 갱신하지 않습니다.

HMR 서버는 대시보드의 조회 경로와 세션 생성·삭제만 자신이 시작한 fixture로 전달합니다.
입력 Host·Origin을 검사한 뒤 fixture Origin으로 대응시키고, fixture 자신의 세션
cookie만 전달하며 직접 로컬 HTTP agent를 사용합니다. Bearer 헤더는 세션 생성 시에만
전달합니다. Backend URL을 입력받거나 관리
변경·continuation 제어·모델 요청·임의 파일을 중계하지 않습니다. 기존 직접 정적
fixture 테스트는 원래 API Origin 경계를 별도로 검증합니다.

Ctrl+C 또는 감독용 stdin 종료 시 두 서버를 정리합니다. Fixture가 예상 밖에 종료되면
DevDemo도 실패로 종료하며 구성·빌드·전송 오류 때문에 합성 모드를 선택하지 않습니다.
두 모드의 기본 포트는 43142이므로 기존 launcher를 종료한 뒤 전환하거나 다른 포트를
명시적으로 선택합니다. Fixture마다 새로운 임시 감사 저장소를 소유하며 모드 간 인증
상태를 공유하지 않습니다.

기존 `web_fixture --assets ...` 정적 실행도 유지합니다. `--api-only`와 `--assets`는
함께 사용할 수 없습니다. 같은 선택형 `--preset all|usage`를 두 실행 방식에 적용할 수
있습니다. 제품 관리 API에 추가되는 기능이 아니라 개발 fixture의 옵션입니다.
