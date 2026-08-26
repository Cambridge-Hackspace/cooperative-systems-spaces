<template>
  <div class="modal-overlay" @click="closeModal">
    <div class="modal-content bg-base-100 text-base-content" @click.stop>
      <div class="modal-header bg-gradient-to-br from-primary via-secondary to-primary">
        <div>
          <h3 class="font">Set Up Training for {{ tool.name }}</h3>
          <p class="subtitle font-bold">Create a comprehensive training program for this tool</p>
        </div>
        <button class="close-btn" @click="closeModal">&times;</button>
      </div>

      <div class="modal-body">
        <!-- Step 1: Training Overview -->
        <div v-if="currentStep === 1" class="setup-step">
          <h4>Step 1: Training Overview</h4>
          <p>
            Let's set up the training requirements for <strong>{{ tool.name }}</strong
            >. This will determine who can access this tool and what they need to learn.
          </p>

          <div class="form-group">
            <label class="checkbox-label">
              <input v-model="trainingConfig.requiresTraining" type="checkbox" class="checkbox" />
              <span class="checkbox-text text-secondary"
                >&nbsp; This tool requires training before use</span
              >
            </label>
            <div class="help-text text-accent">
              If checked, users will need to complete training steps before they can access this
              tool.
            </div>
          </div>

          <div v-if="trainingConfig.requiresTraining" class="training-explanation">
            <div class="info-box bg-secondary text-secondary-content">
              <h5 class="font-bold">What happens when training is required?</h5>
              <ul>
                <li>Tool will show as "Training Required" for untrained users</li>
                <li>Users must complete all training steps to access the tool</li>
                <li>
                  Training can include safety orientation, skill assessments, and certifications
                </li>
                <li>Instructors can track progress and issue certifications</li>
              </ul>
            </div>
          </div>
        </div>

        <!-- Step 2: Training Steps Configuration -->
        <div v-if="currentStep === 2" class="setup-step">
          <h4>Step 2: Configure Training Steps</h4>
          <p>Define the training steps users must complete. Steps will be completed in order.</p>

          <div class="training-steps-config">
            <div v-for="(step, index) in trainingConfig.steps" :key="index" class="step-config">
              <div class="step-header">
                <h5>Step {{ index + 1 }}</h5>
                <button
                  v-if="trainingConfig.steps.length > 1"
                  class="btn btn-sm btn-danger"
                  @click="removeStep(index)"
                >
                  Remove
                </button>
              </div>

              <div class="step-form">
                <div class="form-group">
                  <label>Step Title:</label>
                  <input
                    v-model="step.step_name"
                    type="text"
                    class="form-control input"
                    :placeholder="`e.g., ${getStepTitleSuggestion(index)}`"
                    required
                  />
                </div>

                <div class="form-group">
                  <label>Description:</label>
                  <textarea
                    v-model="step.description"
                    class="form-control input"
                    rows="2"
                    :placeholder="getStepDescriptionSuggestion(index)"
                    required
                  ></textarea>
                </div>

                <div class="form-row">
                  <div class="form-group">
                    <label>Assessment Type:</label>
                    <select v-model="step.assessment_type" class="form-control select">
                      <option value="observation_only">Observation Only</option>
                      <option value="practical">Practical Test</option>
                      <option value="written">Written Test</option>
                      <option value="both">Practical + Written</option>
                    </select>
                  </div>

                  <div v-if="step.assessment_type !== 'observation_only'" class="form-group">
                    <label>Passing Score (%):</label>
                    <input
                      v-model.number="step.passing_score"
                      type="number"
                      class="form-control input"
                      min="1"
                      max="100"
                      placeholder="80"
                    />
                  </div>
                </div>

                <div class="form-row">
                  <div class="form-group">
                    <label>Certification Expires After (days):</label>
                    <input
                      v-model.number="step.expiry_days"
                      type="number"
                      class="form-control input"
                      min="1"
                      placeholder="365 (leave blank for no expiration)"
                    />
                  </div>

                  <div class="form-group">
                    <label class="checkbox-label">
                      <input v-model="step.is_active" type="checkbox" class="checkbox" />
                      <span class="checkbox-text">Active</span>
                    </label>
                  </div>
                </div>
              </div>
            </div>

            <button class="btn btn-secondary" @click="addStep">+ Add Another Step</button>
          </div>
        </div>

        <!-- Step 3: Prerequisites Setup -->
        <div v-if="currentStep === 3" class="setup-step">
          <h4>Step 3: Prerequisites (Optional)</h4>
          <p>Set up dependencies between training steps if needed.</p>

          <div v-if="trainingConfig.steps.length < 2" class="info-message">
            <p>
              Prerequisites are only available when you have multiple training steps. Each step can
              require previous steps to be completed first.
            </p>
          </div>

          <div v-else class="prerequisites-config">
            <div
              v-for="(step, stepIndex) in trainingConfig.steps.slice(1)"
              :key="stepIndex + 1"
              class="prerequisite-config"
            >
              <h5>{{ step.step_name }} (Step {{ stepIndex + 2 }})</h5>
              <div class="form-group">
                <label>Required Prerequisites:</label>
                <div class="checkbox-group">
                  <label
                    v-for="(prereqStep, prereqIndex) in trainingConfig.steps.slice(
                      0,
                      stepIndex + 1
                    )"
                    :key="prereqIndex"
                    class="checkbox-label"
                  >
                    <input
                      v-model="step.prerequisites"
                      type="checkbox"
                      :value="prereqIndex"
                      class="checkbox"
                    />
                    <span class="checkbox-text">
                      Step {{ prereqIndex + 1 }}: {{ prereqStep.step_name }}
                    </span>
                  </label>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Step 4: Review and Confirm -->
        <div v-if="currentStep === 4" class="setup-step">
          <h4>Step 4: Review Training Setup</h4>
          <p>Review your training configuration before creating the training steps.</p>

          <div class="review-section bg-secondary text-secondary-content">
            <div class="review-item"><strong>Tool:</strong> {{ tool.name }}</div>
            <div class="review-item">
              <strong>Requires Training:</strong>
              <span :class="trainingConfig.requiresTraining ? 'text-success' : 'text-muted'">
                {{ trainingConfig.requiresTraining ? 'Yes' : 'No' }}
              </span>
            </div>

            <div v-if="trainingConfig.requiresTraining" class="review-item">
              <strong>Training Steps:</strong>
              <div class="steps-review">
                <div v-for="(step, index) in trainingConfig.steps" :key="index" class="step-review">
                  <div class="step-title">Step {{ index + 1 }}: {{ step.step_name }}</div>
                  <div class="step-details">
                    <div>{{ step.description }}</div>
                    <div class="step-meta">
                      <span>{{ formatAssessmentType(step.assessment_type) }}</span>
                      <span v-if="step.passing_score">• {{ step.passing_score }}% required</span>
                      <span v-if="step.expiry_days">• Expires in {{ step.expiry_days }} days</span>
                      <span :class="step.is_active ? 'text-success' : 'text-muted'">
                        • {{ step.is_active ? 'Active' : 'Inactive' }}
                      </span>
                    </div>
                    <div
                      v-if="step.prerequisites && step.prerequisites.length > 0"
                      class="prerequisites-info"
                    >
                      Prerequisites:
                      <span v-for="(prereqIndex, i) in step.prerequisites" :key="prereqIndex">
                        Step {{ prereqIndex + 1
                        }}{{ i < step.prerequisites.length - 1 ? ', ' : '' }}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div v-if="error" class="error-message">
          {{ error }}
        </div>

        <!-- Navigation -->
        <div class="modal-footer">
          <div class="step-indicator">
            <span
              v-for="step in totalSteps"
              :key="step"
              class="step-dot"
              :class="{ active: step === currentStep, completed: step < currentStep }"
            ></span>
          </div>

          <div class="navigation-buttons">
            <button v-if="currentStep > 1" class="btn btn-secondary" @click="previousStep">
              Previous
            </button>

            <button class="btn btn-outline" @click="closeModal">Cancel</button>

            <button
              v-if="currentStep < totalSteps"
              class="btn btn-primary"
              :disabled="!canProceedToNextStep"
              @click="nextStep"
            >
              Next
            </button>

            <button
              v-if="currentStep === totalSteps"
              class="btn btn-success"
              :disabled="loading || !canCreateSetup"
              @click="createTrainingSetup"
            >
              {{ loading ? 'Creating...' : 'Create Training Setup' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { toolsApi, trainingApi } from '../utils/api'
import { AssessmentType, CreateTrainingStepRequest, Tool } from '../types'

interface TrainingStepConfig {
  step_name: string // Changed from 'title' to match backend
  description: string
  assessment_type: AssessmentType
  passing_score?: number
  expiry_days?: number
  is_active: boolean
  prerequisites: number[]
}

interface TrainingConfig {
  requiresTraining: boolean
  steps: TrainingStepConfig[]
}

interface Props {
  tool: Tool
}

const props = defineProps<Props>()

const emit = defineEmits<{
  close: []
  created: []
}>()

// State
const loading = ref(false)
const error = ref('')
const currentStep = ref(1)
const totalSteps = 4

const trainingConfig = reactive<TrainingConfig>({
  requiresTraining: true,
  steps: [
    {
      step_name: '',
      description: '',
      assessment_type: AssessmentType.Practical,
      passing_score: 80,
      expiry_days: undefined,
      is_active: true,
      prerequisites: [],
    },
  ],
})

// Computed
const canProceedToNextStep = computed(() => {
  switch (currentStep.value) {
    case 1:
      return true // Always can proceed from overview
    case 2:
      return trainingConfig.steps.every((step) => step.step_name.trim() && step.description.trim())
    case 3:
      return true // Prerequisites are optional
    default:
      return true
  }
})

const canCreateSetup = computed(() => {
  return (
    trainingConfig.requiresTraining &&
    trainingConfig.steps.length > 0 &&
    trainingConfig.steps.every((step) => step.step_name.trim() && step.description.trim())
  )
})

// Methods
const closeModal = () => {
  emit('close')
}

const nextStep = () => {
  if (currentStep.value < totalSteps) {
    currentStep.value++
  }
}

const previousStep = () => {
  if (currentStep.value > 1) {
    currentStep.value--
  }
}

const addStep = () => {
  trainingConfig.steps.push({
    step_name: '',
    description: '',
    assessment_type: AssessmentType.Practical,
    passing_score: 80,
    expiry_days: undefined,
    is_active: true,
    prerequisites: [],
  })
}

const removeStep = (index: number) => {
  if (trainingConfig.steps.length > 1) {
    trainingConfig.steps.splice(index, 1)
  }
}

const getStepTitleSuggestion = (index: number): string => {
  const suggestions = [
    'Safety Orientation',
    'Basic Operation Training',
    'Advanced Techniques',
    'Maintenance Certification',
  ]
  return suggestions[index] || `Training Step ${index + 1}`
}

const getStepDescriptionSuggestion = (index: number): string => {
  const suggestions = [
    'Learn safety procedures and protective equipment requirements',
    'Master basic operation and common techniques',
    'Advanced skills and complex projects',
    'Maintenance procedures and troubleshooting',
  ]
  return suggestions[index] || `Description for training step ${index + 1}`
}

const formatAssessmentType = (type: AssessmentType): string => {
  const types = {
    practical: 'Practical Assessment',
    written: 'Written Test',
    both: 'Practical + Written',
    observation_only: 'Observation Only',
  }
  return types[type] || type
}

const createTrainingSetup = async () => {
  try {
    loading.value = true
    error.value = ''

    // First, update the tool to require training
    if (trainingConfig.requiresTraining) {
      await toolsApi.updateTool(props.tool.id, {
        requires_training: true,
      })

      // Create each training step
      const createdSteps: any[] = []

      for (let i = 0; i < trainingConfig.steps.length; i++) {
        const stepConfig = trainingConfig.steps[i]

        const stepRequest: CreateTrainingStepRequest = {
          tool_id: props.tool.id,
          step_number: i + 1,
          step_name: stepConfig.step_name,
          description: stepConfig.description,
          assessment_type: stepConfig.assessment_type,
          passing_score: stepConfig.passing_score,
          expiry_days: stepConfig.expiry_days,
          is_active: stepConfig.is_active,
        }

        const response = await trainingApi.createTrainingStep(stepRequest)

        if (response.success && response.data) {
          createdSteps.push(response.data)
        } else {
          throw new Error(response.error || `Failed to create training step ${i + 1}`)
        }
      }

      // Create prerequisites if any
      for (let i = 0; i < trainingConfig.steps.length; i++) {
        const stepConfig = trainingConfig.steps[i]

        if (stepConfig.prerequisites && stepConfig.prerequisites.length > 0) {
          for (const prereqIndex of stepConfig.prerequisites) {
            await trainingApi.addTrainingPrerequisite({
              training_step_id: createdSteps[i].id,
              prerequisite_step_id: createdSteps[prereqIndex].id,
            })
          }
        }
      }
    } else {
      // Just update tool to not require training
      await toolsApi.updateTool(props.tool.id, {
        requires_training: false,
      })
    }

    emit('created')
    closeModal()
  } catch (err: any) {
    error.value = err.message || 'Failed to create training setup'
  } finally {
    loading.value = false
  }
}
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
  font-size: 1.5rem;
}

.subtitle {
  margin: 0;
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
  min-height: 400px;
}

.setup-step h4 {
  margin-bottom: 0.5rem;
}

.setup-step > p {
  margin-bottom: 1.5rem;
}

.form-group {
  margin-bottom: 1rem;
}

.form-row {
  display: flex;
  gap: 1rem;
}

.form-row .form-group {
  flex: 1;
}

.form-group label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  font-weight: normal;
}

.checkbox {
  width: 18px;
  height: 18px;
}

.checkbox-text {
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

.help-text {
  font-size: 0.8rem;
  margin-top: 0.25rem;
}

.training-explanation {
  margin-top: 1rem;
}

.info-box {
  border: 1px solid #bee5eb;
  border-radius: 4px;
  padding: 1rem;
}

.info-box h5 {
  margin: 0 0 0.5rem 0;
  color: #0c5460;
}

.info-box ul {
  margin: 0;
  color: #0c5460;
}

.training-steps-config {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.step-config {
  border: 1px solid #e1e5e9;
  border-radius: 6px;
  padding: 1rem;
}

.step-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
}

.step-header h5 {
  margin: 0;
  //color: #2c3e50;
}

.step-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.checkbox-group {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.info-message {
  background: #fff3cd;
  border: 1px solid #ffeaa7;
  border-radius: 4px;
  padding: 1rem;
  color: #856404;
}

.prerequisites-config {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.prerequisite-config {
  border: 1px solid #e1e5e9;
  border-radius: 6px;
  padding: 1rem;
}

.prerequisite-config h5 {
  margin: 0 0 1rem 0;
}

.review-section {
  border-radius: 6px;
  padding: 1rem;
}

.review-item {
  margin-bottom: 1rem;
  padding-bottom: 1rem;
  border-bottom: 1px solid #e1e5e9;
}

.review-item:last-child {
  margin-bottom: 0;
  padding-bottom: 0;
  border-bottom: none;
}

.steps-review {
  margin-top: 0.5rem;
}

.step-review {
  border: 1px solid #e1e5e9;
  border-radius: 4px;
  padding: 1rem;
  margin-bottom: 0.5rem;
}

.step-title {
  font-weight: 500;
  margin-bottom: 0.5rem;
}

.step-details {
  //color: #6c757d;
  font-size: 0.9rem;
}

.step-meta {
  margin-top: 0.5rem;
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.prerequisites-info {
  margin-top: 0.5rem;
  font-size: 0.85rem;
  color: #007bff;
}

.text-success {
  color: #28a745 !important;
}

.text-muted {
  color: #6c757d !important;
}

.modal-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-top: 1rem;
  border-top: 1px solid #e1e5e9;
  margin-top: 1rem;
}

.step-indicator {
  display: flex;
  gap: 0.5rem;
}

.step-dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: #e1e5e9;
  transition: background-color 0.2s;
}

.step-dot.active {
  background: #007bff;
}

.step-dot.completed {
  background: #28a745;
}

.navigation-buttons {
  display: flex;
  gap: 1rem;
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

  .form-row {
    flex-direction: column;
  }

  .navigation-buttons {
    flex-direction: column;
  }

  .btn {
    width: 100%;
  }

  .modal-footer {
    flex-direction: column;
    gap: 1rem;
  }
}
</style>
