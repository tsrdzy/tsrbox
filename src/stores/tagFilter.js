import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { useStore } from '../composables/useStore'

/**
 * 标签筛选全局状态：
 * 侧边栏（App.vue）负责展示与选择，首页（HomeView）负责按选择过滤模型。
 */
export const useTagFilterStore = defineStore('tagFilter', () => {
  const { state } = useStore()
  const selectedTags = ref([])

  /** 当前库中实际被模型使用的标签及数量 */
  const allTagsInUse = computed(() => {
    const map = new Map()
    for (const model of state.models) {
      for (const tag of model.tags || []) {
        map.set(tag, (map.get(tag) || 0) + 1)
      }
    }
    return [...map.entries()]
      .map(([name, count]) => ({ name, count }))
      .sort((a, b) => a.name.localeCompare(b.name))
  })

  // 标签失效（模型被删除 / 标签被移除）时自动清掉对应筛选，避免列表被永久过滤为空
  watch(allTagsInUse, (list) => {
    const available = new Set(list.map((item) => item.name))
    const next = selectedTags.value.filter((name) => available.has(name))
    if (next.length !== selectedTags.value.length) {
      selectedTags.value = next
    }
  })

  function toggleTag(name) {
    selectedTags.value = selectedTags.value.includes(name)
      ? selectedTags.value.filter((item) => item !== name)
      : [...selectedTags.value, name]
  }

  function clearTags() {
    selectedTags.value = []
  }

  return { selectedTags, allTagsInUse, toggleTag, clearTags }
})
