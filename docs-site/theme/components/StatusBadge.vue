<script setup lang="ts">
withDefaults(defineProps<{ status?: 'development' | 'preview' | 'supported' }>(), { status: 'development' });
</script>
<template><span class="ds-badge" :data-status="status"><span aria-hidden="true">{{ status === 'supported' ? '✓' : '◈' }}</span><slot /></span></template>
