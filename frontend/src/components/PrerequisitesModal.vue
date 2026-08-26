<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <div>
          <h3>Manage Prerequisites</h3>
          <p class="subtitle">{{ step?.step_name }}</p>
        </div>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <div class="modal-body">
        <!-- Current Prerequisites -->
        <div class="section">
          <h4>Current Prerequisites</h4>
          <div v-if="prerequisites.length === 0" class="empty-state">
            No prerequisites defined for this training step.
          </div>
          <div v-else class="prerequisites-list">
            <div v-for="prereq in prerequisites" :key="prereq.id" class="prerequisite-item">
              <div class="prerequisite-info">
                <strong>Step {{ prereq.step_number }}: {{ prereq.step_name }}</strong>
                <p class="prerequisite-description">{{ prereq.description }}</p>
              </div>
              <button
                class="btn btn-danger btn-sm"
                :disabled="loading"
                @click="removePrerequisite(prereq.id)"
              >
                Remove
              </button>
            </div>
          </div>
        </div>

        <!-- Add New Prerequisite -->
        <div class="section">
          <h4>Add Prerequisite</h4>
          <form class="add-form" @submit.prevent="addPrerequisite">
            <div class="form-group">
              <label for="prerequisite">Select Training Step:</label>
              <select
                id="prerequisite"
                v-model="selectedPrerequisite"
                class="form-control"
                required
              >
                <option value="">Choose a training step</option>
                <option
                  v-for="availableStep in availableSteps"
                  :key="availableStep.id"
                  :value="availableStep.id"
                >
                  {{ getToolName(availableStep.tool_id) }} - Step {{ availableStep.step_number }}:
                  {{ availableStep.step_name }}
                </option>
              </select>
            </div>
            <button
              type="submit"
              class="btn btn-primary"
              :disabled="loading || !selectedPrerequisite"
            >
              {{ loading ? 'Adding...' : 'Add Prerequisite' }}
            </button>
          </form>
        </div>

        <!-- Training Flow Visualization -->
        <div class="section">
          <h4>Training Flow</h4>
          <div class="flow-visualization">
            <div v-if="prerequisites.length === 0" class="flow-item current">
              <div class="flow-step">
                {{ step?.step_name }}
              </div>
            </div>
            <template v-else>
              <div
                v-for="(prereq, index) in sortedPrerequisites"
                :key="prereq.id"
                class="flow-item"
              >
                <div class="flow-step prerequisite">
                  Step {{ prereq.step_number }}: {{ prereq.step_name }}
                </div>
                <div v-if="index < sortedPrerequisites.length - 1 || step" class="flow-arrow">
                  ↓
                </div>
              </div>
              <div v-if="step" class="flow-item current">
                <div class="flow-step">Step {{ step.step_number }}: {{ step.step_name }}</div>
              </div>
            </template>
          </div>
        </div>

        <div v-if="error" class="error-message">
          {{ error }}
        </div>

        <div class="modal-footer">
          <button type="button" class="btn btn-secondary" @click="$emit('close')">Close</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { trainingApi } from '../utils/api'
import type { TrainingStep } from '../types'

interface Props {
  step: TrainingStep | null
  allSteps: TrainingStep[]
}

const props = defineProps<Props>()

const emit = defineEmits<{
  close: []
  updated: []
}>()

const loading = ref(false)
const error = ref('')
const prerequisites = ref<TrainingStep[]>([])
const selectedPrerequisite = ref('')

const availableSteps = computed(() => {
  if (!props.step) return []

  // Filter out the current step and steps that are already prerequisites
  const existingPrereqIds = prerequisites.value.map((p) => p.id)
  return props.allSteps.filter(
    (step) => step.id !== props.step?.id && !existingPrereqIds.includes(step.id)
  )
})

const sortedPrerequisites = computed(() => {
  return [...prerequisites.value].sort((a, b) => a.step_number - b.step_number)
})

const loadPrerequisites = async () => {
  if (!props.step) return

  try {
    loading.value = true
    const response = await trainingApi.getTrainingPrerequisites(props.step.id)
    if (response.success && response.data) {
      prerequisites.value = response.data
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load prerequisites'
  } finally {
    loading.value = false
  }
}

const addPrerequisite = async () => {
  if (!props.step || !selectedPrerequisite.value) return

  loading.value = true
  try {
    const response = await trainingApi.addTrainingPrerequisite({
      training_step_id: props.step.id,
      prerequisite_step_id: selectedPrerequisite.value,
    })

    if (response.success) {
      await loadPrerequisites()
      selectedPrerequisite.value = ''
      emit('updated')
    } else {
      error.value = response.error || 'Failed to add prerequisite'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to add prerequisite'
  } finally {
    loading.value = false
  }
}

const removePrerequisite = async (prerequisiteId: string) => {
  if (!confirm('Are you sure you want to remove this prerequisite?')) return

  loading.value = true
  try {
    const response = await trainingApi.removeTrainingPrerequisite(prerequisiteId)

    if (response.success) {
      await loadPrerequisites()
      emit('updated')
    } else {
      error.value = response.error || 'Failed to remove prerequisite'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to remove prerequisite'
  } finally {
    loading.value = false
  }
}

const getToolName = (toolId: string): string => {
  // This would ideally come from a tools lookup, but for now we'll use the tool_id
  // In a real implementation, you might pass tools as a prop or fetch them
  return `Tool ${toolId.slice(0, 8)}...`
}

watch(
  () => props.step,
  (newStep) => {
    if (newStep) {
      void loadPrerequisites()
    }
  },
  { immediate: true }
)

onMounted(() => {
  if (props.step) {
    void loadPrerequisites()
  }
})
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal-content {
  background: white;
  border-radius: 8px;
  max-width: 800px;
  width: 90%;
  max-height: 90vh;
  overflow-y: auto;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 1.5rem;
  border-bottom: 1px solid #e1e5e9;
}

.modal-header h3 {
  margin: 0 0 0.25rem 0;
  color: #2c3e50;
}

.subtitle {
  margin: 0;
  color: #7f8c8d;
  font-size: 0.9rem;
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: #6c757d;
  padding: 0;
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.close-btn:hover {
  color: #2c3e50;
}

.modal-body {
  padding: 1.5rem;
}

.section {
  margin-bottom: 2rem;
}

.section h4 {
  color: #2c3e50;
  margin-bottom: 1rem;
}

.empty-state {
  color: #7f8c8d;
  font-style: italic;
  padding: 1rem;
  text-align: center;
  background: #f8f9fa;
  border-radius: 4px;
}

.prerequisites-list {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.prerequisite-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem;
  border: 1px solid #e5e5e5;
  border-radius: 4px;
}

.prerequisite-info {
  flex-grow: 1;
}

.prerequisite-info strong {
  color: #2c3e50;
  display: block;
  margin-bottom: 0.25rem;
}

.prerequisite-description {
  margin: 0;
  color: #7f8c8d;
  font-size: 0.9rem;
}

.add-form {
  display: flex;
  gap: 1rem;
  align-items: end;
}

.form-group {
  flex-grow: 1;
}

.form-group label {
  display: block;
  margin-bottom: 0.5rem;
  color: #2c3e50;
  font-weight: 500;
}

.form-control {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 0.9rem;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.flow-visualization {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 1rem;
  background: #f8f9fa;
  border-radius: 4px;
}

.flow-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  margin: 0.5rem 0;
}

.flow-step {
  padding: 1rem 1.5rem;
  border-radius: 8px;
  text-align: center;
  font-weight: 500;
  max-width: 300px;
}

.flow-step.prerequisite {
  background: #e8f4fd;
  border: 2px solid #3498db;
  color: #2980b9;
}

.flow-step:not(.prerequisite) {
  background: #d5f4e6;
  border: 2px solid #27ae60;
  color: #27ae60;
}

.flow-arrow {
  font-size: 1.5rem;
  color: #7f8c8d;
  margin: 0.5rem 0;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 1rem;
  padding-top: 1rem;
  border-top: 1px solid #e1e5e9;
  margin-top: 1rem;
}

.btn {
  padding: 0.5rem 1rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.9rem;
  transition: all 0.2s;
}

.btn-sm {
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-primary {
  background: #007bff;
  color: white;
  border-color: #007bff;
}

.btn-primary:hover:not(:disabled) {
  background: #0056b3;
  border-color: #0056b3;
}

.btn-secondary {
  background: #6c757d;
  color: white;
  border-color: #6c757d;
}

.btn-secondary:hover {
  background: #545b62;
  border-color: #545b62;
}

.btn-danger {
  background: #dc3545;
  color: white;
  border-color: #dc3545;
}

.btn-danger:hover:not(:disabled) {
  background: #c82333;
  border-color: #bd2130;
}

.error-message {
  background: #f8d7da;
  color: #721c24;
  padding: 0.75rem;
  border-radius: 4px;
  margin-top: 1rem;
  border: 1px solid #f5c6cb;
}

@media (max-width: 768px) {
  .modal-content {
    width: 95%;
    margin: 1rem;
  }

  .add-form {
    flex-direction: column;
    align-items: stretch;
  }

  .prerequisite-item {
    flex-direction: column;
    align-items: stretch;
    gap: 1rem;
  }
}
</style>
