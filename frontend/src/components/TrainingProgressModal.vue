<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <div>
          <h3>Training Progress</h3>
          <p class="subtitle">{{ step?.step_name }}</p>
        </div>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <div class="modal-body">
        <div v-if="loading" class="loading">Loading progress data...</div>

        <div v-else>
          <!-- Summary Stats -->
          <div class="stats-section">
            <div class="stat-card">
              <div class="stat-number">{{ totalUsers }}</div>
              <div class="stat-label">Total Users</div>
            </div>
            <div class="stat-card">
              <div class="stat-number">{{ completedUsers }}</div>
              <div class="stat-label">Completed</div>
            </div>
            <div class="stat-card">
              <div class="stat-number">{{ inProgressUsers }}</div>
              <div class="stat-label">In Progress</div>
            </div>
            <div class="stat-card">
              <div class="stat-number">{{ failedUsers }}</div>
              <div class="stat-label">Failed</div>
            </div>
          </div>

          <!-- Progress List -->
          <div class="progress-section">
            <h4>User Progress</h4>

            <!-- Filters -->
            <div class="filters">
              <select v-model="statusFilter" class="filter-select" @change="applyFilters">
                <option value="">All Statuses</option>
                <option value="completed">Completed</option>
                <option value="in_progress">In Progress</option>
                <option value="failed">Failed</option>
                <option value="not_started">Not Started</option>
                <option value="expired">Expired</option>
              </select>

              <input
                v-model="searchQuery"
                type="text"
                placeholder="Search users..."
                class="search-input"
                @input="applyFilters"
              />
            </div>

            <div v-if="filteredProgress.length === 0" class="empty-state">
              <p v-if="allProgress.length === 0">No users have started this training step yet.</p>
              <p v-else>No users match the current filters.</p>
            </div>

            <div v-else class="progress-list">
              <div
                v-for="progress in paginatedProgress"
                :key="progress.id"
                class="progress-item"
                :class="getProgressStatusClass(progress.status)"
              >
                <div class="user-info">
                  <div class="user-name">
                    {{ progress.user?.full_name || progress.user?.username || 'Unknown User' }}
                  </div>
                  <div class="user-details">
                    <span class="user-email">{{ progress.user?.email }}</span>
                    <span class="user-role">{{ progress.user?.role }}</span>
                  </div>
                </div>

                <div class="progress-details">
                  <div class="status-info">
                    <span class="status-badge" :class="getStatusBadgeClass(progress.status)">
                      {{ formatStatus(progress.status) }}
                    </span>
                    <div class="dates">
                      <div v-if="progress.started_at" class="date-info">
                        Started: {{ formatDate(progress.started_at) }}
                      </div>
                      <div v-if="progress.completed_at" class="date-info">
                        Completed: {{ formatDate(progress.completed_at) }}
                      </div>
                      <div
                        v-if="progress.expires_at"
                        class="date-info expiry"
                        :class="{ expired: isExpired(progress.expires_at) }"
                      >
                        {{ isExpired(progress.expires_at) ? 'Expired:' : 'Expires:' }}
                        {{ formatDate(progress.expires_at) }}
                      </div>
                    </div>
                  </div>

                  <div v-if="progress.assessment_score !== null" class="assessment-info">
                    <div class="score">Score: {{ progress.assessment_score }}%</div>
                    <div
                      v-if="step?.passing_score"
                      class="passing-indicator"
                      :class="{ passed: progress.assessment_score >= step.passing_score }"
                    >
                      {{ progress.assessment_score >= step.passing_score ? 'PASS' : 'FAIL' }} ({{
                        step.passing_score
                      }}% required)
                    </div>
                  </div>

                  <div v-if="progress.instructor" class="instructor-info">
                    <div class="instructor">
                      Instructor:
                      {{ progress.instructor.full_name || progress.instructor.username }}
                    </div>
                  </div>

                  <div v-if="progress.notes" class="notes">
                    <strong>Notes:</strong> {{ progress.notes }}
                  </div>
                </div>
              </div>
            </div>

            <!-- Pagination -->
            <div v-if="totalPages > 1" class="pagination">
              <button
                :disabled="currentPage === 1"
                class="btn btn-sm btn-secondary"
                @click="currentPage = Math.max(1, currentPage - 1)"
              >
                Previous
              </button>

              <span class="page-info">
                Page {{ currentPage }} of {{ totalPages }} ({{ filteredProgress.length }} total)
              </span>

              <button
                :disabled="currentPage === totalPages"
                class="btn btn-sm btn-secondary"
                @click="currentPage = Math.min(totalPages, currentPage + 1)"
              >
                Next
              </button>
            </div>
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
import type { TrainingStep, UserTrainingProgress, TrainingStatus } from '../types'

interface Props {
  step: TrainingStep | null
}

const props = defineProps<Props>()

defineEmits<{
  close: []
}>()

const loading = ref(false)
const error = ref('')
const allProgress = ref<UserTrainingProgress[]>([])
const statusFilter = ref('')
const searchQuery = ref('')
const currentPage = ref(1)
const pageSize = 10

const filteredProgress = computed(() => {
  let filtered = allProgress.value

  // Filter by status
  if (statusFilter.value) {
    filtered = filtered.filter((p) => p.status === statusFilter.value)
  }

  // Filter by search query
  if (searchQuery.value.trim()) {
    const search = searchQuery.value.toLowerCase()
    filtered = filtered.filter((p) => {
      const user = p.user
      return (
        user?.full_name?.toLowerCase().includes(search) ||
        user?.username?.toLowerCase().includes(search) ||
        user?.email?.toLowerCase().includes(search) ||
        p.notes?.toLowerCase().includes(search)
      )
    })
  }

  return filtered
})

const paginatedProgress = computed(() => {
  const start = (currentPage.value - 1) * pageSize
  const end = start + pageSize
  return filteredProgress.value.slice(start, end)
})

const totalPages = computed(() => {
  return Math.ceil(filteredProgress.value.length / pageSize)
})

// Stats computed properties
const totalUsers = computed(() => allProgress.value.length)
const completedUsers = computed(
  () => allProgress.value.filter((p) => p.status === 'completed').length
)
const inProgressUsers = computed(
  () => allProgress.value.filter((p) => p.status === 'in_progress').length
)
const failedUsers = computed(() => allProgress.value.filter((p) => p.status === 'failed').length)

const loadProgress = () => {
  if (!props.step) return

  loading.value = true
  try {
    // In a real implementation, you'd have an endpoint to get all user progress for a training step
    // For now, we'll simulate this
    error.value =
      'Training progress viewing is not fully implemented yet. This would show all users who have attempted this training step.'
  } catch (err: any) {
    error.value = err.message || 'Failed to load training progress'
  } finally {
    loading.value = false
  }
}

const applyFilters = () => {
  currentPage.value = 1
}

const getProgressStatusClass = (status: TrainingStatus): string => {
  return `progress-${status.replace('_', '-')}`
}

const getStatusBadgeClass = (status: TrainingStatus): string => {
  switch (status) {
    case 'completed':
      return 'badge-success'
    case 'in_progress':
      return 'badge-warning'
    case 'failed':
      return 'badge-danger'
    case 'expired':
      return 'badge-dark'
    default:
      return 'badge-secondary'
  }
}

const formatStatus = (status: TrainingStatus): string => {
  const statusMap = {
    not_started: 'Not Started',
    in_progress: 'In Progress',
    completed: 'Completed',
    failed: 'Failed',
    expired: 'Expired',
  }
  return statusMap[status] || status
}

const formatDate = (dateString: string): string => {
  return new Date(dateString).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

const isExpired = (dateString: string): boolean => {
  return new Date(dateString) <= new Date()
}

watch(
  () => props.step,
  (newStep) => {
    if (newStep) {
      void loadProgress()
    }
  },
  { immediate: true }
)

onMounted(() => {
  if (props.step) {
    void loadProgress()
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
  background: var(--fallback-b1, oklch(var(--b1) / 1));
  border-radius: 8px;
  max-width: 1000px;
  width: 90%;
  max-height: 90vh;
  overflow-y: auto;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 1.5rem;
  border-bottom: 1px solid var(--fallback-b3, oklch(var(--b3) / 1));
}

.modal-header h3 {
  margin: 0 0 0.25rem 0;
  color: var(--fallback-bc, oklch(var(--bc) / 1));
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
  color: var(--fallback-bc, oklch(var(--bc) / 1));
}

.modal-body {
  padding: 1.5rem;
}

.loading {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
}

.stats-section {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
  gap: 1rem;
  margin-bottom: 2rem;
}

.stat-card {
  background: var(--fallback-b2, oklch(var(--b2) / 1));
  padding: 1rem;
  border-radius: 6px;
  text-align: center;
  border: 1px solid var(--fallback-b3, oklch(var(--b3) / 1));
}

.stat-number {
  font-size: 1.5rem;
  font-weight: bold;
  color: var(--fallback-bc, oklch(var(--bc) / 1));
  margin-bottom: 0.25rem;
}

.stat-label {
  color: #6c757d;
  font-size: 0.9rem;
}

.progress-section h4 {
  color: var(--fallback-bc, oklch(var(--bc) / 1));
  margin-bottom: 1rem;
}

.filters {
  display: flex;
  gap: 1rem;
  margin-bottom: 1rem;
  align-items: center;
}

.filter-select,
.search-input {
  padding: 0.5rem;
  border: 1px solid var(--fallback-b3, oklch(var(--b3) / 1));
  border-radius: 4px;
  font-size: 0.9rem;
}

.search-input {
  flex: 1;
  max-width: 300px;
}

.empty-state {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
  background: var(--fallback-b2, oklch(var(--b2) / 1));
  border-radius: 4px;
}

.progress-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.progress-item {
  border: 1px solid var(--fallback-b3, oklch(var(--b3) / 1));
  border-radius: 6px;
  padding: 1rem;
  transition: border-color 0.2s;
}

.progress-item:hover {
  border-color: #007bff;
}

.progress-completed {
  border-left: 4px solid #28a745;
  background: rgba(40, 167, 69, 0.05);
}

.progress-in-progress {
  border-left: 4px solid #ffc107;
  background: rgba(255, 193, 7, 0.05);
}

.progress-failed {
  border-left: 4px solid #dc3545;
  background: rgba(220, 53, 69, 0.05);
}

.progress-expired {
  border-left: 4px solid #6c757d;
  background: rgba(108, 117, 125, 0.05);
}

.user-info {
  margin-bottom: 0.5rem;
}

.user-name {
  font-weight: 500;
  color: var(--fallback-bc, oklch(var(--bc) / 1));
  margin-bottom: 0.25rem;
}

.user-details {
  display: flex;
  gap: 1rem;
  font-size: 0.9rem;
  color: #6c757d;
}

.progress-details {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.status-info {
  display: flex;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}

.status-badge {
  padding: 0.25rem 0.5rem;
  border-radius: 12px;
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
}

.badge-success {
  background: #d4edda;
  color: #155724;
}

.badge-warning {
  background: #fff3cd;
  color: #856404;
}

.badge-danger {
  background: #f8d7da;
  color: #721c24;
}

.badge-dark {
  background: #d6d8db;
  color: #383d41;
}

.badge-secondary {
  background: #e2e3e5;
  color: #383d41;
}

.dates {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
}

.date-info {
  font-size: 0.8rem;
  color: #6c757d;
}

.expiry.expired {
  color: #dc3545;
  font-weight: 500;
}

.assessment-info {
  display: flex;
  gap: 1rem;
  align-items: center;
}

.score {
  font-weight: 500;
  color: var(--fallback-bc, oklch(var(--bc) / 1));
}

.passing-indicator {
  font-size: 0.8rem;
  font-weight: 600;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
}

.passing-indicator.passed {
  background: #d4edda;
  color: #155724;
}

.passing-indicator:not(.passed) {
  background: #f8d7da;
  color: #721c24;
}

.instructor-info {
  font-size: 0.9rem;
  color: #6c757d;
}

.notes {
  font-size: 0.9rem;
  color: #495057;
  background: var(--fallback-b2, oklch(var(--b2) / 1));
  padding: 0.5rem;
  border-radius: 4px;
}

.pagination {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 1rem;
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--fallback-b3, oklch(var(--b3) / 1));
}

.page-info {
  font-size: 0.9rem;
  color: #6c757d;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--fallback-b3, oklch(var(--b3) / 1));
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

.btn-secondary {
  background: #6c757d;
  color: white;
  border-color: #6c757d;
}

.btn-secondary:hover:not(:disabled) {
  background: #545b62;
  border-color: #545b62;
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

  .filters {
    flex-direction: column;
    align-items: stretch;
  }

  .search-input {
    max-width: none;
  }

  .status-info,
  .dates {
    flex-direction: column;
    gap: 0.25rem;
  }

  .assessment-info {
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
