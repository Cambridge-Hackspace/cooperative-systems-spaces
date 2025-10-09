<template>
  <div class="training-view">
    <div class="training-header">
      <h2>Training Management</h2>
      <div class="training-controls">
        <div class="filters">
          <select v-model="selectedTool" @change="loadTrainingSteps">
            <option value="">All Tools</option>
            <option v-for="tool in tools" :key="tool.id" :value="tool.id">
              {{ tool.name }}
            </option>
          </select>
          
          <input 
            v-model="searchQuery" 
            @input="loadTrainingSteps"
            type="text" 
            placeholder="Search training steps..." 
            class="search-input"
          />
        </div>
        
        <button 
          v-if="canManageTraining" 
          @click="showCreateModal = true"
          class="btn btn-primary"
        >
          Add Training Step
        </button>
      </div>
    </div>

    <div class="loading" v-if="loading">Loading training steps...</div>
    
    <div class="error" v-else-if="error">
      {{ error }}
    </div>

    <div class="training-content" v-else>
      <div class="training-steps-list" v-if="trainingSteps.length">
        <div 
          v-for="step in trainingSteps" 
          :key="step.id"
          class="step-card"
        >
          <div class="step-header">
            <h3>{{ step.step_name }}</h3>
            <div class="step-meta">
              <span class="tool-name">{{ getToolName(step.tool_id) }}</span>
              <span class="step-number">Step {{ step.step_number }}</span>
            </div>
          </div>

          <div class="step-body">
            <p>{{ step.description }}</p>
            <div class="step-details">
              <span class="assessment-type">{{ formatAssessmentType(step.assessment_type) }}</span>
              <span v-if="step.passing_score" class="passing-score">
                Passing Score: {{ step.passing_score }}%
              </span>
              <span v-if="step.expiry_days" class="expiry">
                Expires: {{ step.expiry_days }} days
              </span>
              <span :class="step.is_active ? 'status-active' : 'status-inactive'">
                {{ step.is_active ? 'Active' : 'Inactive' }}
              </span>
            </div>
          </div>

          <div class="step-actions" v-if="canManageTraining">
            <button @click="editStep(step)" class="btn btn-sm btn-secondary">
              Edit
            </button>
            <button @click="managePrerequisites(step)" class="btn btn-sm btn-info">
              Prerequisites
            </button>
            <button @click="viewProgress(step)" class="btn btn-sm btn-primary">
              View Progress
            </button>
            <button 
              @click="deleteStep(step)" 
              class="btn btn-sm btn-danger"
            >
              Delete
            </button>
          </div>
        </div>
      </div>

      <div class="no-training" v-else>
        <p>No training steps found.</p>
        <button 
          v-if="canManageTraining" 
          @click="showCreateModal = true"
          class="btn btn-primary"
        >
          Create First Training Step
        </button>
      </div>
    </div>

    <!-- Create Training Step Modal -->
    <CreateTrainingStepModal
      v-if="showCreateModal"
      :tools="tools"
      @close="showCreateModal = false"
      @created="onStepCreated"
    />

    <!-- Edit Training Step Modal -->
    <EditTrainingStepModal
      v-if="editingStep"
      :step="editingStep"
      :tool="tools.find(t => t.id === editingStep?.tool_id) || tools[0]"
      :existing-steps="trainingSteps"
      @close="editingStep = null"
      @step-updated="onStepUpdated"
    />

    <!-- Prerequisites Management Modal -->
    <PrerequisitesModal
      v-if="managingPrerequisites"
      :step="managingPrerequisites"
      :all-steps="trainingSteps"
      @close="managingPrerequisites = null"
      @updated="onPrerequisitesUpdated"
    />

    <!-- Progress View Modal -->
    <TrainingProgressModal
      v-if="viewingProgress"
      :step="viewingProgress"
      @close="viewingProgress = null"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useAuthStore } from '../stores/auth'
import { trainingApi, toolsApi } from '../utils/api'
import type {
  TrainingStep,
  Tool,
  AssessmentType
} from '../types'
import CreateTrainingStepModal from '../components/CreateTrainingStepModal.vue'
import EditTrainingStepModal from '../components/EditTrainingStepModal.vue'
import PrerequisitesModal from '../components/PrerequisitesModal.vue'
import TrainingProgressModal from '../components/TrainingProgressModal.vue'

const auth = useAuthStore()

// State
const trainingSteps = ref<TrainingStep[]>([])
const tools = ref<Tool[]>([])
const loading = ref(false)
const error = ref('')
const searchQuery = ref('')
const selectedTool = ref('')
const showCreateModal = ref(false)
const editingStep = ref<TrainingStep | null>(null)
const managingPrerequisites = ref<TrainingStep | null>(null)
const viewingProgress = ref<TrainingStep | null>(null)

// Computed
const canManageTraining = computed(() => {
  const userRole = auth.user?.role?.toLowerCase()
  return userRole === 'staff' || userRole === 'admin'
})

// Methods
const loadTools = async () => {
  try {
    const response = await toolsApi.getTools()
    if (response.success && response.data) {
      tools.value = response.data
    }
  } catch (err) {
    console.error('Error loading tools:', err)
  }
}

const loadTrainingSteps = async () => {
  try {
    loading.value = true
    error.value = ''
    
    const query: any = {}
    if (selectedTool.value) query.tool_id = selectedTool.value
    
    const response = await trainingApi.getTrainingSteps(query)
    
    if (response.success && response.data) {
      let steps = response.data
      
      // Filter by search query if provided
      if (searchQuery.value.trim()) {
        const search = searchQuery.value.toLowerCase()
        steps = steps.filter(step => 
          step.step_name.toLowerCase().includes(search) ||
          step.description.toLowerCase().includes(search)
        )
      }
      
      trainingSteps.value = steps.sort((a, b) => {
        // Sort by tool name, then by step number
        const toolA = getToolName(a.tool_id)
        const toolB = getToolName(b.tool_id)
        if (toolA !== toolB) return toolA.localeCompare(toolB)
        return a.step_number - b.step_number
      })
    } else {
      error.value = response.error || 'Failed to load training steps'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load training steps'
  } finally {
    loading.value = false
  }
}

const getToolName = (toolId: string): string => {
  const tool = tools.value.find(t => t.id === toolId)
  return tool ? tool.name : 'Unknown Tool'
}

const formatAssessmentType = (type: AssessmentType): string => {
  const types = {
    practical: 'Practical',
    written: 'Written',
    both: 'Both',
    observation_only: 'Observation'
  }
  return types[type] || type
}

const editStep = (step: TrainingStep) => {
  editingStep.value = step
}

const managePrerequisites = (step: TrainingStep) => {
  managingPrerequisites.value = step
}

const viewProgress = (step: TrainingStep) => {
  viewingProgress.value = step
}

const deleteStep = async (step: TrainingStep) => {
  if (!confirm(`Are you sure you want to delete the training step "${step.step_name}"?`)) return
  
  try {
    const response = await trainingApi.deleteTrainingStep(step.id)
    if (response.success) {
      await loadTrainingSteps()
    } else {
      error.value = response.error || 'Failed to delete training step'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to delete training step'
  }
}

const onStepCreated = () => {
  showCreateModal.value = false
  loadTrainingSteps()
}

const onStepUpdated = () => {
  editingStep.value = null
  loadTrainingSteps()
}

const onPrerequisitesUpdated = () => {
  managingPrerequisites.value = null
  loadTrainingSteps()
}

// Lifecycle
onMounted(async () => {
  await loadTools()
  await loadTrainingSteps()
})
</script>

<style scoped>
.training-view {
  padding: 2rem;
  max-width: 1400px;
  margin: 0 auto;
}

.training-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 2rem;
  flex-wrap: wrap;
  gap: 1rem;
}

.training-header h2 {
  margin: 0;
  color: #2c3e50;
}

.training-controls {
  display: flex;
  gap: 1rem;
  align-items: center;
  flex-wrap: wrap;
}

.filters {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.filters select, .search-input {
  padding: 0.5rem;
  border: 1px solid #ddd;
  border-radius: 4px;
  font-size: 0.9rem;
}

.search-input {
  min-width: 200px;
}

.btn {
  padding: 0.5rem 1rem;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.9rem;
  transition: background-color 0.2s;
}

.btn-sm {
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}

.btn-primary {
  background-color: #3498db;
  color: white;
}

.btn-primary:hover {
  background-color: #2980b9;
}

.btn-secondary {
  background-color: #95a5a6;
  color: white;
}

.btn-secondary:hover {
  background-color: #7f8c8d;
}

.btn-info {
  background-color: #3498db;
  color: white;
}

.btn-info:hover {
  background-color: #2980b9;
}

.btn-danger {
  background-color: #e74c3c;
  color: white;
}

.btn-danger:hover:not(:disabled) {
  background-color: #c0392b;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.loading, .error {
  text-align: center;
  padding: 2rem;
  font-size: 1.1rem;
}

.error {
  color: #e74c3c;
  background-color: #fdf2f2;
  border: 1px solid #fbb6b6;
  border-radius: 4px;
}

.training-steps-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.step-card {
  background: white;
  border: 1px solid #e1e5e9;
  border-radius: 8px;
  padding: 1.5rem;
  transition: border-color 0.2s;
}

.step-card:hover {
  border-color: #3498db;
}

.step-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 1rem;
}

.step-header h3 {
  margin: 0;
  color: #2c3e50;
}

.step-meta {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.25rem;
  font-size: 0.9rem;
}

.tool-name {
  color: #3498db;
  font-weight: 500;
}

.step-number {
  color: #6c757d;
}

.step-body {
  margin-bottom: 1rem;
}

.step-body p {
  margin: 0 0 1rem 0;
  color: #6c757d;
  line-height: 1.5;
}

.step-details {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
  font-size: 0.9rem;
  color: #6c757d;
}

.assessment-type {
  background: #e3f2fd;
  color: #1565c0;
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.8rem;
}

.passing-score {
  background: #fff3e0;
  color: #e65100;
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.8rem;
}

.expiry {
  background: #f3e5f5;
  color: #7b1fa2;
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.8rem;
}

.status-active {
  background: #e8f5e8;
  color: #2e7d32;
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.8rem;
}

.status-inactive {
  background: #fafafa;
  color: #757575;
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.8rem;
}

.step-actions {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.no-training {
  text-align: center;
  padding: 3rem;
  color: #6c757d;
}

.no-training p {
  margin-bottom: 1rem;
  font-size: 1.1rem;
}

@media (max-width: 768px) {
  .training-header {
    flex-direction: column;
    align-items: stretch;
  }
  
  .training-controls {
    flex-direction: column;
    align-items: stretch;
  }
  
  .filters {
    flex-wrap: wrap;
  }
  
  .step-header {
    flex-direction: column;
    align-items: stretch;
    gap: 1rem;
  }
  
  .step-meta {
    align-items: flex-start;
  }
  
  .step-details {
    flex-direction: column;
    gap: 0.5rem;
  }
  
  .step-actions {
    justify-content: stretch;
  }
  
  .btn {
    flex: 1;
  }
}
</style>
