<template>
  <div class="dashboard-container">
    <div class="container" v-loading="loading">
      <div class="tableBar">
        <label style="margin-right: 5px">员工姓名：</label>
        <el-input
          v-model="input"
          placeholder="请输入员工姓名"
          style="width: 15%"
          clearable
          @clear="initFun"
          @keyup.enter.native="initFun"
        />
        <el-button class="normal-btn continue" @click="initFun"
          >查询</el-button
        >
        <el-button
          type="primary"
          style="float: right"
          @click="addEmployeeHandle('add')"
        >
          + 添加员工
        </el-button>
      </div>
      <el-table
        :data="tableData"
        stripe
        v-if="tableData.length"
        class="tableBox"
      >
        <el-table-column prop="name" label="员工姓名" />
        <el-table-column label="工号">
          <template slot-scope="scope">{{ scope.row.employeeNo || '—' }}</template>
        </el-table-column>
        <el-table-column prop="username" label="账号" />
        <el-table-column prop="phone" label="手机号" />
        <el-table-column label="账号状态">
          <template slot-scope="scope">
            <div
              class="tableColumn-status"
              :class="{ 'stop-use': String(scope.row.status) === '0' }"
            >
              {{ String(scope.row.status) === '0' ? '禁用' : '启用' }}
            </div>
          </template>
        </el-table-column>
        <el-table-column label="最后操作时间" min-width="168">
          <template slot-scope="scope">{{ scope.row.updateTime || '—' }}</template>
        </el-table-column>
        <el-table-column label="操作" width="160" align="center">
          <template slot-scope="scope">
            <el-button
              type="text"
              size="small"
              class="blueBug"
              :class="{ 'disabled-text': scope.row.username === 'admin' }"
              :disabled="scope.row.username === 'admin'"
              @click="addEmployeeHandle(scope.row.id, scope.row.username)"
            >
              修改
            </el-button>
            <el-button
              :disabled="scope.row.username === 'admin'"
              type="text"
              size="small"
              class="non"
              :class="{
                'disabled-text': scope.row.username === 'admin',
                blueBug: scope.row.status == '0',
                delBut: scope.row.status != '0',
              }"
              @click="statusHandle(scope.row)"
            >
              {{ scope.row.status == '1' ? '禁用' : '启用' }}
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      <Empty v-else-if="!loading" :is-search="isSearch" />
      <el-pagination
        class="pageList"
        :page-sizes="[10, 20, 30, 40]"
        :page-size="pageSize"
        :current-page="page"
        layout="total, sizes, prev, pager, next, jumper"
        :total="counts"
        @size-change="handleSizeChange"
        @current-change="handleCurrentChange"
      />
    </div>
  </div>
</template>

<script lang="ts">
import { Component, Vue } from 'vue-property-decorator'
import HeadLable from '@/components/HeadLable/index.vue'
import { getEmployeeList, enableOrDisableEmployee } from '@/api/employee'
import { UserModule } from '@/store/modules/user'
import InputAutoComplete from '@/components/InputAutoComplete/index.vue'
import Empty from '@/components/Empty/index.vue'

@Component({
  name: 'Employee',
  components: {
    HeadLable,
    InputAutoComplete,
    Empty,
  },
})
export default class extends Vue {
  private input = ''
  private searchName = ''
  private loading = false
  private requestSequence = 0
  private counts: number = 0
  private page: number = 1
  private pageSize: number = 10
  private tableData = []

  get isSearch() {
    return this.searchName.length > 0
  }

  created() {
    this.init()
  }

  initProp(val) {
    this.input = val
    this.initFun()
  }

  initFun() {
    this.searchName = this.input.trim()
    this.page = 1
    this.init()
  }

  get userName() {
    return UserModule.username
  }

  private async init() {
    const request = ++this.requestSequence
    this.loading = true
    const params = {
      page: this.page,
      pageSize: this.pageSize,
      name: this.searchName || undefined,
    }
    try {
      const { data } = await getEmployeeList(params)
      if (request !== this.requestSequence) return
      if (String(data.code) !== '1') {
        this.$message.error(data.msg || '员工查询失败')
        return
      }
      this.tableData = data.data.records
      this.counts = data.data.total
    } catch (error) {
      if (request === this.requestSequence) {
        const response = error.response && error.response.data
        this.$message.error((response && response.msg) || '员工查询失败')
      }
    } finally {
      if (request === this.requestSequence) this.loading = false
    }
  }

  // 添加
  private addEmployeeHandle(st: string, username: string) {
    if (st === 'add') {
      this.$router.push({ path: '/employee/add' })
    } else {
      if (username === 'admin') {
        return
      }
      this.$router.push({ path: '/employee/add', query: { id: st } })
    }
  }

  //状态修改
  private statusHandle(row: any) {
    if (row.username === 'admin') {
      return
    }
    const id = row.id
    const status = Number(row.status) === 1 ? 0 : 1
    this.$confirm('确认调整该账号的状态?', '提示', {
      confirmButtonText: '确定',
      cancelButtonText: '取消',
      type: 'warning',
    }).then(() => {
      enableOrDisableEmployee({ id, status })
        .then((res) => {
          if (res.data.code === 1) {
            this.$message.success('账号状态更改成功！')
            this.init()
          } else {
            this.$message.error(res.data.msg || '账号状态更改失败')
          }
        })
        .catch((err) => {
          this.$message.error('请求出错了：' + err.message)
        })
    }, () => undefined)
  }

  private handleSizeChange(val: any) {
    this.pageSize = val
    this.page = 1
    this.init()
  }

  private handleCurrentChange(val: any) {
    this.page = val
    this.init()
  }
}
</script>

<style lang="scss" scoped>
.disabled-text {
  color: #bac0cd !important;
}
</style>
