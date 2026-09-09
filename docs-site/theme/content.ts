import { computed } from 'vue';
import { useData } from 'vitepress';
import catalogue from '../../.local/docs-site/catalogue.json';

export const site = catalogue;
export const labels = {
  en: {
    docs: 'Documentation', api: 'API reference', project: 'Project',
    stage: 'In development', note: 'Implementation note', caution: 'Limit',
    preview: 'Working tree preview', source: 'Source', language: 'Page language',
    notices: 'Web dependency notices', table: 'Supported behavior',
    gateway: 'Responses gateway', consumer: 'Your application', upstream: 'Declared upstream',
    transport: 'Transport · routes · credentials', tools: 'Tools · approvals · history',
    routeCaption: 'One local Responses entry point. Each route selects its upstream API and credentials.',
    ownershipCaption: 'The application runs tools and owns approvals and history. The gateway carries model requests and responses.',
    wire: 'JSON / SSE', local: 'Local by design',
    toolCaption: 'A tool call returns to the application. The application approves and runs the tool, then sends its result in a new model request.',
    toolSteps: ['Send a model request', 'Receive the tool call', 'Approve and run the tool in the application', 'Send the tool result in the next request', 'Receive the model response'],
  },
  ko: {
    docs: '문서', api: 'API 레퍼런스', project: '프로젝트',
    stage: '개발 중', note: '구현 참고', caution: '제한 사항',
    preview: '작업 트리 미리보기', source: '소스', language: '페이지 언어',
    notices: '웹 의존성 고지', table: '지원 범위',
    gateway: 'Responses 게이트웨이', consumer: '애플리케이션', upstream: '선언한 업스트림',
    transport: '전송 · 경로 · 자격 증명', tools: '도구 · 승인 · 이력',
    routeCaption: '하나의 로컬 Responses 진입점에서 각 경로의 업스트림 API와 자격 증명을 선택합니다.',
    ownershipCaption: '애플리케이션이 도구를 실행하고 승인과 이력을 소유합니다. 게이트웨이는 모델 요청과 응답을 전달합니다.',
    wire: 'JSON / SSE', local: '로컬 실행을 기본으로',
    toolCaption: '도구 호출은 애플리케이션으로 돌아옵니다. 애플리케이션이 승인하고 실행한 뒤 새 모델 요청으로 결과를 전달합니다.',
    toolSteps: ['모델 요청 전송', '도구 호출 수신', '애플리케이션에서 도구 승인·실행', '다음 요청으로 도구 결과 전송', '모델 응답 수신'],
  },
};
export type Locale = keyof typeof labels;
export function useContent() {
  const { lang } = useData();
  const locale = computed(() => (lang.value.split('-')[0] in labels ? lang.value.split('-')[0] : 'en') as Locale);
  return { locale, ui: computed(() => labels[locale.value]), pages: computed(() => site.pages.filter(p => p.locale === locale.value)) };
}
