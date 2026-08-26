<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal" @click.stop>
      <div class="modal-header">
        <h3>Record Training Session</h3>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <div class="modal-body">
        <form @submit.prevent="submitForm">
          <div class="form-row">
            <div class="form-group">
              <label for="tool">Tool</label>
              <input id="tool" :value="tool ? tool.name : ''" class="form-control" disabled />
            </div>

            <div class="form-group">
              <label for="trainee">Trainee</label>
              <select id="trainee" v-model="formData.trainee_user_id" class="form-control" required>
                <option value="">Select trainee...</option>
                <option v-for="user in users" :key="user.id" :value="user.id">
                  {{ user.full_name || user.username }} ({{ user.email }})
                </option>
              </select>
            </div>
          </div>

          <div class="form-row">
            <div class="form-group">
              <label for="training_date">Training Date</label>
              <input
                id="training_date"
                v-model="formData.training_date"
                type="date"
                class="form-control"
                :max="today"
                required
              />
            </div>

            <div class="form-group">
              <label for="completion_status">Completion Status</label>
              <select
                id="completion_status"
                v-model="formData.completion_status"
                class="form-control"
                required
              >
                <option value="">Select status...</option>
                <option value="completed">Completed</option>
                <option value="partial">Partial</option>
                <option value="failed">Failed</option>
              </select>
            </div>
          </div>

          <div class="form-group">
            <label for="minutes_trained">Duration (Minutes)</label>
            <input
              id="minutes_trained"
              v-model.number="formData.minutes_trained"
              type="number"
              min="1"
              max="480"
              class="form-control"
              placeholder="e.g. 60"
            />
            <small class="form-text">Optional - How long was the training session?</small>
          </div>

          <div class="form-group">
            <label for="skills_covered">Skills Covered</label>
            <div class="skills-input">
              <input
                v-model="newSkill"
                type="text"
                class="form-control"
                placeholder="Type a skill and press Enter"
                @keydown.enter.prevent="addSkill"
              />
              <button
                type="button"
                class="btn btn-sm btn-secondary"
                :disabled="!newSkill.trim()"
                @click="addSkill"
              >
                Add
              </button>
            </div>

            <div
              v-if="formData.skills_covered && formData.skills_covered.length > 0"
              class="skills-list"
            >
              <span
                v-for="(skill, index) in formData.skills_covered"
                :key="index"
                class="skill-tag"
              >
                {{ skill }}
                <button type="button" class="skill-remove" @click="removeSkill(index)">
                  &times;
                </button>
              </span>
            </div>
          </div>

          <div class="form-group">
            <label for="notes">Training Notes</label>
            <textarea
              id="notes"
              v-model="formData.notes"
              class="form-control"
              rows="3"
              placeholder="Notes about the training session, what went well, areas for improvement..."
            ></textarea>
          </div>

          <div class="form-group">
            <label for="next_steps">Next Steps</label>
            <textarea
              id="next_steps"
              v-model="formData.next_steps"
              class="form-control"
              rows="2"
              placeholder="Recommended next steps for the trainee..."
            ></textarea>
          </div>

          <div v-if="error" class="error">{{ error }}</div>

          <div class="modal-actions">
            <button type="button" class="btn btn-secondary" @click="$emit('close')">Cancel</button>
            <button type="submit" :disabled="submitting" class="btn btn-primary">
              {{ submitting ? 'Recording...' : 'Record Training' }}
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { trainerApi, userApi } from '../utils/api'
import type { Tool, CreateTrainingRecordRequest, TrainingCompletionStatus, User } from '../types'

interface Props {
  tool?: Tool
}

const props = defineProps<Props>()
const emit = defineEmits<{
  close: []
  recorded: []
}>()

// State
const users = ref<User[]>([])
const loading = ref(false)
const submitting = ref(false)
const error = ref('')
const newSkill = ref('')

const formData = ref<CreateTrainingRecordRequest>({
  tool_id: props.tool?.id || '',
  trainee_user_id: '',
  training_date: new Date().toISOString().split('T')[0],
  completion_status: 'completed' as TrainingCompletionStatus,
  minutes_trained: undefined,
  skills_covered: [],
  notes: '',
  next_steps: '',
})

// Computed
const today = computed(() => {
  return new Date().toISOString().split('T')[0]
})

// Methods
const loadUsers = async () => {
  try {
    loading.value = true
    error.value = ''

    const response = await userApi.getAllUsers()

    if (response.success && response.data?.items) {
      users.value = response.data.items.filter((user) => user.is_active)
    } else {
      error.value = response.error || 'Failed to load users'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load users'
  } finally {
    loading.value = false
  }
}

const addSkill = () => {
  const skill = newSkill.value.trim()
  if (skill && !formData.value.skills_covered?.includes(skill)) {
    if (!formData.value.skills_covered) {
      formData.value.skills_covered = []
    }
    formData.value.skills_covered.push(skill)
    newSkill.value = ''
  }
}

const removeSkill = (index: number) => {
  formData.value.skills_covered?.splice(index, 1)
}

const submitForm = async () => {
  try {
    submitting.value = true
    error.value = ''

    // Clean up the data
    const requestData: CreateTrainingRecordRequest = {
      ...formData.value,
      skills_covered: formData.value.skills_covered?.length
        ? formData.value.skills_covered
        : undefined,
      notes: formData.value.notes || undefined,
      next_steps: formData.value.next_steps || undefined,
    }

    const response = await trainerApi.createTrainingRecord(requestData)

    if (response.success) {
      emit('recorded')
    } else {
      error.value = response.error || 'Failed to record training session'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to record training session'
  } finally {
    submitting.value = false
  }
}

// Lifecycle
onMounted(() => {
  void loadUsers()
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

.modal {
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  width: 90%;
  max-width: 600px;
  max-height: 90vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.modal-header {
  padding: 1.5rem;
  border-bottom: 1px solid #e1e5e9;
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: #f8f9fa;
}

.modal-header h3 {
  margin: 0;
  color: #2c3e50;
  font-size: 1.25rem;
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
  border-radius: 50%;
  transition: all 0.2s;
}

.close-btn:hover {
  background: #e9ecef;
  color: #495057;
}

.modal-body {
  padding: 1.5rem;
  overflow-y: auto;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1rem;
  margin-bottom: 1rem;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group:last-child {
  margin-bottom: 0;
}

label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
  color: #2c3e50;
}

.form-control {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 1rem;
  transition: border-color 0.2s;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.form-control:disabled {
  background-color: #e9ecef;
  opacity: 1;
}

.form-text {
  font-size: 0.875rem;
  color: #6c757d;
  margin-top: 0.25rem;
}

.skills-input {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
}

.skills-input .form-control {
  margin-bottom: 0;
}

.skills-list {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.skill-tag {
  background: #e9ecef;
  border: 1px solid #ced4da;
  border-radius: 16px;
  padding: 0.25rem 0.75rem;
  font-size: 0.875rem;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.skill-remove {
  background: none;
  border: none;
  color: #6c757d;
  cursor: pointer;
  padding: 0;
  font-size: 1rem;
  line-height: 1;
  transition: color 0.2s;
}

.skill-remove:hover {
  color: #dc3545;
}

.error {
  background: #f8d7da;
  color: #721c24;
  padding: 0.75rem;
  border-radius: 4px;
  border: 1px solid #f5c6cb;
  margin-bottom: 1rem;
}

.modal-actions {
  display: flex;
  gap: 0.75rem;
  justify-content: flex-end;
  margin-top: 1.5rem;
  padding-top: 1rem;
  border-top: 1px solid #e1e5e9;
}

.btn {
  padding: 0.75rem 1.5rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 1rem;
  transition: all 0.2s;
  text-decoration: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-sm {
  padding: 0.5rem 1rem;
  font-size: 0.875rem;
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

.btn-secondary:hover:not(:disabled) {
  background: #545b62;
  border-color: #545b62;
}

select.form-control,
textarea.form-control {
  cursor: pointer;
}

textarea.form-control {
  resize: vertical;
  min-height: 80px;
}

@media (max-width: 768px) {
  .modal {
    width: 95%;
    margin: 1rem;
  }

  .modal-header,
  .modal-body {
    padding: 1rem;
  }

  .form-row {
    grid-template-columns: 1fr;
  }

  .skills-input {
    flex-direction: column;
  }

  .modal-actions {
    flex-direction: column;
  }

  .btn {
    width: 100%;
  }
}
</style>
