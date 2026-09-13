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
