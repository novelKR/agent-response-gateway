<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue';
import { useData } from 'vitepress';
import { useContent } from '../content';
import StatusBadge from './StatusBadge.vue';
const { frontmatter } = useData();
const { ui } = useContent();
const markdown = computed(() => typeof frontmatter.value.copyMarkdown === 'string' ? frontmatter.value.copyMarkdown : null);
const state = ref<'idle' | 'copying' | 'copied' | 'failed'>('idle');
const feedback = computed(() => state.value === 'idle' ? '' : ui.value[state.value]);
let timer: ReturnType<typeof setTimeout> | undefined;
let operation = 0;
function reset() {
  operation++;
  clearTimeout(timer);
  state.value = 'idle';
}
watch(() => frontmatter.value.sourcePath, reset, { flush: 'sync' });
onUnmounted(reset);
async function copyPage() {
  if (markdown.value === null || state.value === 'copying') return;
  reset();
  const current = operation;
  state.value = 'copying';
  try {
    await navigator.clipboard.writeText(markdown.value);
    if (current !== operation) return;
    state.value = 'copied';
    timer = setTimeout(reset, 2000);
  } catch {
    if (current !== operation) return;
    state.value = 'failed';
  }
}
</script>
<template>
  <div class="ds-toolbar" v-if="markdown !== null">
    <StatusBadge>{{ ui.stage }}</StatusBadge>
    <div class="ds-copy-page">
      <button type="button" :title="ui.copyMarkdown" :aria-disabled="state === 'copying'" :aria-busy="state === 'copying'" @click="copyPage">
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M15 5V4a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h1" /></svg>
        {{ ui.copyPage }}
      </button>
      <span class="ds-copy-feedback" :data-state="state" role="status">{{ feedback }}</span>
    </div>
  </div>
</template>
