<script setup>
import { computed } from 'vue';
import { jsonTokens } from './json-tokens.mjs';
const props = defineProps({ value: { default: undefined }, emptyText: { type: String, required: true } });
const tokens = computed(() => props.value == null ? null : jsonTokens(JSON.stringify(props.value, null, 2)));
</script>

<template>
  <pre class="json-view"><code v-if="tokens"><span v-for="(token, index) in tokens" :key="index" :class="`json-${token.kind}`">{{ token.text }}</span></code><template v-else>{{ emptyText }}</template></pre>
</template>
