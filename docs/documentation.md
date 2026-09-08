# 공개 문서와 로컬 기록 관리

공개 저장소는 범용 API·실행·배포 계약을 관리한다. 특정 소비자의 이름,
저장소 주소, 로컬 경로, 내부 서비스 구성과 운영 기록은 공개 문서·예제·
커밋 메시지·태그·CI 로그에 포함하지 않는다. 공개 빌드와 테스트는 별도
로컬 기록 없이 동작해야 한다.

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
