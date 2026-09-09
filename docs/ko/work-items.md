<a id="github-work-items"></a>

# GitHub 작업 단위

[English](../work-items.md) | [한국어](work-items.md)

각 작업은 자신의 PR과 증거를 추적한다. Issue 종료는 공급자 qualification,
소비자 수락이나 릴리스를 뜻하지 않는다. 소비자 소유 작업은 소비자 저장소에서
구현·검증한다.

| 작업 | 마일스톤 | Issue | 선행 작업 |
|---|---|---|---|
| G00 | M0 | [GitHub 작업 기반](https://github.com/novelKR/agent-response-gateway/issues/1) | Baseline |
| G01 | M0 | [라이선스 증거 통합](https://github.com/novelKR/agent-response-gateway/issues/2) | Baseline |
| G02 | M1 | [공개 ref 검사](https://github.com/novelKR/agent-response-gateway/issues/3) | G00 |
| G03 | M1 | [고정 Codex 실행 계약](https://github.com/novelKR/agent-response-gateway/issues/4) | G00 |
| G04 | M1 | [Codex 기준 시험 장치](https://github.com/novelKR/agent-response-gateway/issues/5) | G03 |
| G05 | M2 | [모델 경로·기능 설계](https://github.com/novelKR/agent-response-gateway/issues/6) | G04 |
| G06 | M2 | [경로·기능 admission](https://github.com/novelKR/agent-response-gateway/issues/7) | G05 |
| G07 | M3 | [Messages 요청·응답 codec](https://github.com/novelKR/agent-response-gateway/issues/8) | G06 |
| G08 | M3 | [Messages stream·custom 도구](https://github.com/novelKR/agent-response-gateway/issues/9) | G07 |
| G09 | M3 | [Messages Codex 적합성](https://github.com/novelKR/agent-response-gateway/issues/10) | G08 |
| G10 | M4 | [Chat Completions 요청·응답](https://github.com/novelKR/agent-response-gateway/issues/11) | G09 |
| G11 | M4 | [Chat Completions stream·도구](https://github.com/novelKR/agent-response-gateway/issues/12) | G10 |
| G12 | M4 | [세 프로토콜 적합성](https://github.com/novelKR/agent-response-gateway/issues/13) | G11 |
| G13 | M5 | [범용 내장·접근 계약](https://github.com/novelKR/agent-response-gateway/issues/14) | G09 |
| G14 | M5 | [소비자 런타임 내장](https://github.com/novelKR/agent-response-gateway/issues/15) | G13 |
| G15 | M6 | [연속성·압축·재개 설계](https://github.com/novelKR/agent-response-gateway/issues/16) | G04 |
| G16 | M6 | [승인된 연속성 구현](https://github.com/novelKR/agent-response-gateway/issues/17) | G15, G14 |
| G17 | M6 | [소비자 장기 실행 수락](https://github.com/novelKR/agent-response-gateway/issues/18) | G16 |
| G18 | M7 | [배포 패키지·검증](https://github.com/novelKR/agent-response-gateway/issues/19) | G01, G09 |
| G19 | M7 | [빌드 출처·릴리스 승격](https://github.com/novelKR/agent-response-gateway/issues/20) | G18 |
| G20 | M7 | [소비자 고정 버전 채택](https://github.com/novelKR/agent-response-gateway/issues/21) | G19, G17 |

브랜치는 `codex/m<N>-<purpose>`를 사용하고 merge commit으로 논리적 이력을
보존한다. 설계·운영 승인 경계는 [로드맵](roadmap.md)과
[GitHub 절차](github-workflow.md)를 따른다.
