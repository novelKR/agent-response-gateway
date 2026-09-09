<script setup lang="ts">
import { useContent } from '../content';
withDefaults(defineProps<{ variant?: 'note' | 'caution'; title?: string }>(), { variant: 'note' });
const { ui } = useContent();
</script>
<template><aside class="ds-callout" :data-variant="variant"><div class="ds-callout-title"><span aria-hidden="true">{{ variant === 'caution' ? '△' : 'ⓘ' }}</span>{{ title || ui[variant] }}</div><slot /></aside></template>
