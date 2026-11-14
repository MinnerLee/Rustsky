import { createLocalVue, mount } from '@vue/test-utils'
import VueRouter from 'vue-router'
import ElementUI from 'element-ui'
import Breadcrumb from '@/components/Breadcrumb/index.vue'

const localVue = createLocalVue()
localVue.use(VueRouter)
localVue.use(ElementUI)

const View = { render: h => h('router-view') }

describe('Breadcrumb.vue', () => {
  it('updates nested titles, hides opted-out routes and navigates through a parent link', async () => {
    const router = new VueRouter({
      mode: 'abstract',
      routes: [
        { path: '/dashboard', component: View, meta: { title: '工作台' } },
        {
          path: '/manage', component: View, meta: { title: '管理' },
          children: [{
            path: 'employee', component: View, meta: { title: '员工' },
            children: [
              { path: 'edit', component: View, meta: { title: '编辑' } },
              { path: 'hidden', component: View, meta: { title: '隐藏', breadcrumb: false } }
            ]
          }]
        }
      ]
    })
    await router.push('/dashboard')
    const wrapper = mount(Breadcrumb, { localVue, router })
    const labels = () => wrapper.findAll('.el-breadcrumb__inner').wrappers.map(item => item.text())
    expect(labels()).toEqual(['工作台'])

    await router.push('/manage/employee/edit')
    await wrapper.vm.$nextTick()
    expect(labels()).toEqual(['管理', '员工', '编辑'])
    expect(wrapper.findAll('.el-breadcrumb__inner').at(2).find('a').exists()).toBe(false)

    await router.push('/manage/employee/hidden')
    await wrapper.vm.$nextTick()
    expect(labels()).toEqual(['管理', '员工'])
    await wrapper.find('.el-breadcrumb__inner a').trigger('click')
    expect(router.currentRoute.path).toBe('/manage')
    wrapper.destroy()
  })
})
