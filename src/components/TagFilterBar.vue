<script setup>
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

const props = defineProps({
  tags: { type: Array, default: () => [] },
  modelValue: { type: Array, default: () => [] },
  placeholder: { type: String, default: '' },
  /** bar：横向单行（旧布局）；sidebar：侧边栏两列网格 */
  variant: { type: String, default: 'bar' }
})
const emit = defineEmits(['update:modelValue'])

const { t } = useI18n()

const dialogVisible = ref(false)
const search = ref('')

const orderedTags = computed(() => {
  const selected = new Set(props.modelValue)
  return [...props.tags].sort((a, b) => {
    // 侧边栏固定按名称排序，避免选中后标签位置跳动
    if (props.variant !== 'sidebar') {
      const sa = selected.has(a.name) ? 0 : 1
      const sb = selected.has(b.name) ? 0 : 1
      if (sa !== sb) return sa - sb
    }
    return a.name.localeCompare(b.name)
  })
})

const dialogTags = computed(() => {
  const q = search.value.trim().toLowerCase()
  if (!q) return props.tags
  return props.tags.filter((tag) => tag.name.toLowerCase().includes(q))
})

function isSelected(name) {
  return props.modelValue.includes(name)
}

function toggle(name) {
  const next = props.modelValue.includes(name)
    ? props.modelValue.filter((n) => n !== name)
    : [...props.modelValue, name]
  emit('update:modelValue', next)
}

function clearAll() {
  emit('update:modelValue', [])
}

function removeTag(name) {
  emit('update:modelValue', props.modelValue.filter((n) => n !== name))
}

function openDialog() {
  search.value = ''
  dialogVisible.value = true
}
</script>

<template>
  <div class="tag-filter-bar" :class="variant">
    <div class="tag-filter-scroll">
      <span
        v-for="tag in orderedTags"
        :key="tag.name"
        class="tag-chip"
        :class="{ active: isSelected(tag.name) }"
        :title="`${tag.name} (${tag.count})`"
        @click="toggle(tag.name)"
      >
        <span class="tag-icon">&#xeb2a;</span>
        <span class="tag-name">{{ tag.name }}</span>
        <span class="tag-count">{{ tag.count }}</span>
        <span v-if="isSelected(tag.name)" class="tag-remove" @click.stop="removeTag(tag.name)">×</span>
      </span>
      <span v-if="!orderedTags.length" class="tag-empty">{{ placeholder || t('common.tags') }}</span>
    </div>
    <button class="tag-all-btn" :title="t('common.allTags')" @click="openDialog">
      <span class="tag-all-icon">&#xeb2a;</span>
      <span>{{ t('common.allTags') }}</span>
      <span class="tag-all-count">{{ tags.length }}</span>
    </button>

    <el-dialog v-model="dialogVisible" :title="t('common.allTags')" width="560px" append-to-body>
      <div class="tag-dialog-toolbar">
        <el-input v-model="search" :placeholder="t('common.tagSearch')" clearable size="small" class="tag-search" />
        <span class="tag-dialog-count">{{ t('common.selected') }} {{ modelValue.length }} / {{ tags.length }}</span>
      </div>
      <div class="tag-dialog-body">
        <span
          v-for="tag in dialogTags"
          :key="tag.name"
          class="tag-chip large"
          :class="{ active: isSelected(tag.name) }"
          @click="toggle(tag.name)"
        >
          <span class="tag-icon">&#xeb2a;</span>
          <span class="tag-name">{{ tag.name }}</span>
          <span class="tag-count">{{ tag.count }}</span>
        </span>
        <div v-if="!dialogTags.length" class="tag-dialog-empty">{{ t('common.noMatch') }}</div>
      </div>
      <template #footer>
        <el-button size="small" @click="clearAll">{{ t('common.clearFilters') }}</el-button>
        <el-button size="small" type="primary" @click="dialogVisible = false">{{ t('common.ok') }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.tag-filter-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  max-width: none;
}
.tag-filter-scroll {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  white-space: nowrap;
}
.tag-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--border-soft);
  border-radius: 12px;
  background: var(--bg-mute);
  color: var(--text-2);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
  user-select: none;
}
.tag-chip:hover {
  border-color: var(--primary);
  color: var(--primary);
}
.tag-chip.active {
  background: var(--primary-soft);
  border-color: var(--primary);
  color: var(--primary);
}
.tag-icon {
  font-family: 'iconfont';
  font-size: 11px;
}
.tag-name {
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tag-count {
  font-size: 10px;
  opacity: 0.7;
}
.tag-remove {
  font-size: 13px;
  line-height: 1;
  margin-left: 1px;
}
.tag-empty {
  font-size: 12px;
  color: var(--text-3);
}
.tag-all-btn {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--border-soft);
  border-radius: 13px;
  background: transparent;
  color: var(--text-2);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.tag-all-btn:hover {
  border-color: var(--primary);
  color: var(--primary);
}
.tag-all-icon {
  font-family: 'iconfont';
  font-size: 12px;
}
.tag-all-count {
  font-size: 10px;
  color: var(--primary);
}
.tag-dialog-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}
.tag-search {
  width: 240px;
}
.tag-dialog-count {
  font-size: 12px;
  color: var(--text-3);
}
.tag-dialog-body {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  max-height: 380px;
  overflow-y: auto;
  padding: 4px 2px;
}
.tag-chip.large {
  height: 28px;
  padding: 0 12px;
  font-size: 13px;
}
.tag-dialog-empty {
  width: 100%;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  padding: 24px 0;
}

/* ===== 侧边栏模式：两列标签，每个占一半宽度 ===== */
.tag-filter-bar.sidebar {
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  width: 100%;
}
.tag-filter-bar.sidebar .tag-filter-scroll {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px;
  overflow: visible;
  white-space: normal;
  width: 100%;
}
.tag-filter-bar.sidebar .tag-chip {
  width: 100%;
  min-width: 0;
  height: 26px;
  padding: 0 7px;
  justify-content: flex-start;
  border-radius: 6px;
}
.tag-filter-bar.sidebar .tag-icon {
  display: none;
}
.tag-filter-bar.sidebar .tag-name {
  flex: 1;
  min-width: 0;
  max-width: none;
}
.tag-filter-bar.sidebar .tag-count {
  flex-shrink: 0;
}
.tag-filter-bar.sidebar .tag-all-btn {
  width: 100%;
  justify-content: center;
  height: 26px;
}
.tag-filter-bar.sidebar .tag-empty {
  grid-column: 1 / -1;
  text-align: center;
  padding: 6px 0;
}
</style>
