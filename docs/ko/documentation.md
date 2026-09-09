<a id="public-documentation-and-local-records"></a>

# 공개 문서와 로컬 기록 관리

[English](../documentation.md) | [한국어](documentation.md)

공개 저장소는 범용 API·실행·배포 계약을 관리한다. 특정 소비자의 이름,
저장소 주소, 로컬 경로, 내부 서비스 구성과 운영 기록은 공개 문서·예제·
커밋 메시지·태그·CI 로그에 포함하지 않는다. 공개 빌드와 테스트는 별도
로컬 기록 없이 동작해야 한다.

<a id="independent-local-history"></a>

## 독립적인 로컬 이력

로컬 문서를 같은 작업 폴더에서 관리해야 한다면 `.private/`에 별도 Git
저장소를 둘 수 있다. 부모 `.gitignore`는 이 경로 전체를 제외한다.
submodule로 등록하거나 `.gitmodules`·gitlink·공개 링크로 연결하지 않는다.
부모 저장소의 commit·push는 이 내부 저장소의 이력이나 백업을 포함하지 않는다.
이 디렉터리를 만들지 않아도 공개 프로젝트를 사용할 수 있다.

일반화 전 원본은 내부에서 경로·바이트·해시와 함께 보존하고, 공개 문서에는
재사용 가능한 요구와 계약을 남긴다. 내부 기록은 관련 공개 커밋을 참조할 수
있지만 공개 문서에서는 내부 식별정보를 참조하지 않는다. 제외 규칙은
접근 권한이나 백업을 설정하지 않으므로 내부 원격과 보존 정책은 별도로 관리한다.

<a id="publication-checks"></a>

## 공개 경계 검사

Python 표준 라이브러리 검사기는 일반 파일만 허용한다. 예약된 내부·빌드 경로,
gitlink와 symlink, `.gitmodules`를 거부한다. 기본 검사는 Git index의 실제 blob과
모든 로컬 ref에서 도달 가능한 커밋 이력을 읽는다. 현재 파일을 지웠어도
스테이징된 blob이나 과거 이력에 남은 내용은 검사 대상이다.

```sh
python3 -B scripts/check_public_boundary.py
python3 -B -m unittest discover -s scripts/tests -v
```

아직 최초 커밋이나 스테이징이 없는 로컬 작업에서는 다음 명령으로
무시되지 않은 untracked 파일까지 검사한다. 기본 검사는 빈 index를 통과시키지 않는다.

```sh
python3 -B scripts/check_public_boundary.py --worktree
```

별도의 비공개 식별자 검사가 필요하면 문자열 배열의 JSON 사전을 외부 파일로
전달할 수 있다. 파일명은 예시이며 사전의 내용이나 실제 내부 식별자를
공개 소스·CI에 기록하지 않는다.

```sh
python3 -B scripts/check_public_boundary.py --worktree --private-patterns .private/patterns.json
```

사전 검사는 정확한 UTF-8 문자열 일치를 사용한다. 문맥상 관계 추론이나 모든
변형 표기를 자동 탐지하지 않으므로 최종 공개 diff 검토도 필요하다.
검사 실패 출력에는 일치한 내용이나 내부 경로를 포함하지 않는다.
CI는 전체 이력 checkout으로 기본 검사를 실행하며 비공개 사전을 요구하지 않는다.

<a id="source-archives"></a>

## 소스 압축파일

공개 배포물은 검토한 공개 커밋의 추적 파일에서 만든다. 작업 폴더 전체를
압축하는 명령은 사용하지 않는다. 다음 명령은 커밋된 공개 HEAD가 있을 때 실행한다.

```sh
mkdir -p .local/release
git archive --format=tar.gz --prefix=agent-response-gateway/ --output=.local/release/source.tar.gz HEAD
python3 -B scripts/check_public_boundary.py --archive .local/release/source.tar.gz
```

archive 검사도 예약 경로와 링크·특수 파일을 거부하며, 선택적 사전이 있으면
압축파일의 파일 내용에도 적용한다. 검사기는 파일을 추출하지 않는다.
압축파일 검사 통과는 Corresponding Source 완전성이나 바이너리 재현성을
증명하지 않는다. 그 절차는 [배포 문서](release.md)를 따른다.

검사기는 Git 객체·index·refs를 변경하지 않는다. 큰 입력은 정해진 검사
한도를 넘으면 실패하며, 얕은 clone은 전체 이력을 확보한 뒤 검사해야 한다.

<a id="license-evidence"></a>

## 라이선스 자료

`licensing/`의 정책·패키지 기록·고지 원문은 공개 Git에서 함께 관리한다.
모든 원문에는 공개 출처와 해시를 연결하며, 로컬 캐시 경로나 비공개 계약
기록을 복사하지 않는다. [전용 검사](../../licensing/README.ko.md)는 원본과 선택
기록의 일치를, 이 문서의 공개 경계 검사는 선택한 파일·이력·archive의 공개
가능 경계를 확인한다. 두 검사는 서로를 대체하지 않는다.

`license_audit.py bundle`의 고지 묶음도 `scripts/archive_notices.py`로 tar를
만들어 호스트 메타데이터를 제외하고 위 archive 검사를 적용한다.
공개 소스에 `.private/`가 없어도 라이선스 검사·고지 생성이 가능해야 한다.

<a id="ref-and-local-configuration-scope"></a>

## Ref와 로컬 설정의 검사 범위

commit ref의 전체 도달 이력과 분리된 HEAD의 이력을 검사한다. tree ref는
해당 트리의 경로·일반 파일·내용을 같은 규칙으로 검사한다. 중첩 annotated
tag도 원문·대상 객체 유형을 확인한 뒤 최종 commit 또는 tree를 검사한다.
직접 blob을 가리키는 ref나 tag는 지원하지 않으며 명시적으로 실패한다.
tree ref를 무시하거나 검사 통과를 위해 ref를 삭제하지 않는다.

`.codex/`는 개인 실행 설정용 예약 경로다. index·이력·tree ref·archive에
포함되면 거부한다. 정상적인 tree 객체라는 사실은 그 내용이 공개 가능하다는
뜻이 아니다. 내부 설정을 포함한 로컬 snapshot이 있으면 전체 로컬 검사는
계속 실패할 수 있으며, 실제 공개할 ref와 산출물의 검증 결과와 구분한다.

<a id="translation-maintenance"></a>

## 번역 관리

영문 파일이 편집 기준이다. 루트 문서는 대응 `.ko.md`, `docs/`의 문서는
`docs/ko/`를 사용한다. 법률 원문과 생성 고지는 동일한 기준 증거를 유지하고,
`AGENTS.md`는 하나의 영어 실행 지침으로 둔다. 제목을 바꿀 때도 상호 언어
링크와 명시적 호환 앵커를 유지한다. 명령, 코드 블록, 식별자와 지원 조건을
영문·한국어 사이에 일치시킨다.

[문서 목록](../translations.json)은 고정 ID, 탐색 그룹, 보존할 앵커, 영문·한국어 경로와
검토한 두 파일의 해시를 기록한다. 해시는 변경을 감지하지만 의미 동등성이나
사용자 승인을 증명하지 않는다. 전체 대응 문서를 검토한 뒤 기록한다. 누락되거나
과거 버전인 번역은 명시적으로 실패하며 CI가 검토 기록을 자동 갱신하지 않는다.

```sh
python3.14 -B scripts/check_docs.py
python3.14 -B scripts/check_docs.py record --id documentation
```

두 번째 명령은 해당 문서 쌍을 검토한 편집자가 사용한다. 검토한 ID를 각각
명시한다. 새 유지보수 Markdown에는 한국어 판본과 목록 등록이 필요하다.
검사기는 목록, 절, 기술 표기, 코드, 표 구조, 공통 앵커와 상대 링크를 확인한다.
합성 회귀는 번역 누락·과거 버전·계약 손상을 검사하며 편집 검토를 대체하지 않는다.

<a id="documentation-site-development"></a>

## 문서 사이트 개발

사이트는 커밋한 [npm lock](../../docs-site/package-lock.json)을 사용하며,
VitePress 1.6.4, Node 24.21.0과 npm 11.19.0을 고정한다. Python 3.11+가
필요하고 CI는 Python 3.14를 선택한다. Python 실행 파일 이름이 다르면
DOCS_PYTHON 환경 변수를 설정한다. 저장소 루트에서 다음을 실행한다.

```sh
npm ci --prefix docs-site --ignore-scripts
npm test --prefix docs-site
npm run build --prefix docs-site
python3 -B docs-site/scripts/check-output.py
npm run preview --prefix docs-site
```

미리보기 주소는 `http://127.0.0.1:43140/agent-response-gateway/`다. 수정 후
다시 빌드하고 새로고침한다. 미리보기에는 hot module replacement가 없다.
프로젝트 기준 경로는 `/agent-response-gateway/`이며, 영문은 루트, 한국어는
`/ko/`에 있다. 유지보수 문서는 `/guide/`와 `/ko/guide/`에 배치한다. 도구 모음의
언어 링크는 같은 문서의 대응 페이지로 이동하며 호환 앵커를 유지한다.
로컬 검색은 질의를 브라우저 안에서 처리한다. 검색 인덱스에는 선택한 공개 문서
내용만 포함하며 공급자, 분석 도구나 원격 검색 요청이 필요하지 않다.

문서 목록이 안정적인 ID, 원본 쌍, 분류, 순서와 사이트 경로를 관리한다.
[탐색 메타데이터](../../docs-site/navigation.json)는 언어별 분류 이름, 언어 경로와
API 경로 그림의 데이터를 제공한다. 페이지 제목과 카드 요약은 Markdown의
제목과 첫 설명 문단에서 가져온다. 문서 쌍과 목록의 경로를 추가하고, 필요하면
분류 이름을 추가한다. 기존 경로는 유지된다. 두 언어는 같은 테마와 컴포넌트를
사용한다. 다른 언어를 추가할 때는 편집 검사기와 번역한 UI 표제도 명시적으로
확장해야 한다.

색상, 글꼴, 간격과 폭은 [중앙 토큰](../../docs-site/theme/tokens.css)에서 수정한다.
컴포넌트는 의미 토큰을 참조하며 VitePress 변수도 같은 값에 연결한다. 반응형
규칙은 공식 테마의 기존 분기점을 확장한다. 시스템 글꼴, 로컬로 묶인 테마 아이콘과
직접 만든 SVG 연결선을 사용한다. 콘텐츠 슬롯과 타입이 있는 props가 페이지
내용과 레이아웃을 분리한다. 코드, 사이드바, 페이지 목차, 테마 선택과 검색은
VitePress의 공식 확장 지점을 사용한다.

입력 생성기는 문서 목록에서 선택한 Markdown만 ignored 빌드 디렉터리로 복사한다.
페이지 스크립트, 페이지 스타일과 다른 파일을 포함하는 지시문은 거부한다.
게시 대상이 아닌 소스 링크는 checkout을 복사하지 않고 공개 소스 commit으로
연결한다. 산출물 검사는 전체 페이지 목록, 내부 링크, 앵커, 자산의 출처와 생성
자산을 검증한다. 빌드 manifest는 게시 파일 각각의 SHA-256과 작업 트리의 변경
여부를 기록한다. 이는 무결성 기록이며 attestation은 아니다. 설치 디렉터리,
캐시와 생성 페이지는 Git과 소스 패키지에서 제외한다.

<a id="web-notices-and-development-server-constraints"></a>

## 웹 고지와 개발 서버 제약

웹 의존성은 정적 사이트의 개발 의존성이며 Rust 게이트웨이와 그 라이선스 감사와
구분한다. 빌드는 Rollup의 클라이언트 모듈 목록을 읽어 패키지 버전과 무결성
기록을 npm lock에 대조하고 원문 고지의 바이트를 보존한다.
[검토한 기록](../../docs-site/licensing/dependencies.json)에는 VitePress에 내장된
Lucide/Feather 아이콘도 포함한다. 해당 소스 파일과 라이선스 해시는 별도로
고정한다. DocSearch CSS tarball에는 라이선스가 빠져 있으므로 정확한 공식
소스 태그의 commit에서 MIT 원문 고지를 보존한다. 글꼴 파일과 소셜 아이콘
자산은 배포하지 않는다.

생성한 `web-dependencies.json`, `web-notices.txt`와 변경하지 않은 프로젝트
라이선스 `LICENSE.txt`를 사이트에 함께 제공한다.
변경한 패키지, 라이선스 조건, 원문 고지와 내장 자산을 검토한 뒤 목록을
명시적으로 기록한다.

```sh
npm run build --prefix docs-site -- --record-notices
git diff -- docs-site/licensing
npm run build --prefix docs-site
```

해시 기록은 상용 권리를 승인하지 않는다. 일반 빌드와 CI는 오래된 기록을
거부한다. VitePress 갱신 시 내장 테마 자산도 검토해야 한다. npm 패키지 경계만으로
복사된 모든 자산을 설명할 수는 없다.

선택한 안정 버전은 Vite 5.4.21과 esbuild 0.21.5를 사용한다. npm audit는
영향받는 패키지 항목 3개(보통 2개, 높음 1개)를 보고하며, 개발 서버 관련
권고 4개에 해당한다. [esbuild CORS](https://github.com/advisories/GHSA-67mh-4wv8-2f99),
[최적화한 의존성 경로](https://github.com/advisories/GHSA-4w7w-66w2-5vf9),
[Windows 파일 시스템 경로](https://github.com/advisories/GHSA-fx2h-pf6j-xcff),
[Windows 편집기 실행](https://github.com/advisories/GHSA-v6wh-96g9-6wx3)이다.
이 사이트 구현은 해당 취약점을 수정하지 않는다. 지원하는 명령은 생산 빌드를
수행하고 loopback에 바인딩한 별도의 정적 HTTP 미리보기를 사용한다. Vite 개발
서버, esbuild 서버와 편집기 실행 endpoint는 시작하지 않는다. 이 절차를
`vitepress dev`나 내장 미리보기로 대체하지 않는다. 의존성 갱신에는 새로운
호환성·고지 검토가 필요하며, npm override로 호환되지 않는 Vite 주 버전을
강제로 적용하지 않는다.

<a id="site-verification-and-publication"></a>

## 사이트 검증과 공개

검토 전에 두 언어를 데스크톱, 태블릿과 320픽셀 모바일 폭에서 확인한다.
긴 제목, 표, 키보드 포커스, 테마 변경과 페이지 직접 새로고침을 점검한다.
`authentication`, `인증`, `previous_response_id`, `압축`을 검색한다. 같은 페이지의
언어 전환, 기존 앵커와 404 페이지를 확인한다. 임시 팔레트와 간격 변경이 탐색,
카드, 배지, 표, 코드와 다이어그램에 반영되는지 검사한 뒤 토큰을 복구한다.
합성 테스트는 새 분류와 번역, 금지된 산출물과 미리보기 경로 이탈도 검사한다.

읽기 전용 `docs` CI 작업은 `ci-required`에 포함되며 검증한 정적 산출물을
검토용으로 보관한다. Pages 활성화, 배포 권한과 보호된 배포 환경에는 별도의
공개 결정이 필요하다. 이 결정에 소스 commit, 빌드 manifest와 웹 고지를 제공한다.
승인한 산출물을 재빌드 없이 배포하고 실제 URL을 확인한다. 사이트 복구에는
이전에 검증한 산출물을, 소스 변경 복구에는 되돌리는 PR을 사용한다. 로컬 빌드의
성공이나 산출물 보관은 실제 공개 사이트, 정식 게이트웨이 릴리스나 소비자 운영
수락을 의미하지 않는다.
