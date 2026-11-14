import request from '@/utils/request'

export interface MealCategory {
  id: number
  name: string
}

export interface MealDish {
  dishId: number
  name: string
  price: number
  copies: number
  status?: number
}

export interface MealPayload {
  id?: number
  name: string
  categoryId: number
  price: string
  image: string
  description: string
  status: number
  setmealDishes: Array<{ dishId: number; copies: number }>
}
/**
 *
 * 套餐管理
 *
 **/

// 查询列表数据
export const getSetmealPage = (params: any) => {
  return request({
    url: '/setmeal/page',
    method: 'get',
    params,
  },)
}

// 删除数据接口
export const deleteSetmeal = (ids: string) => {
  return request({
    url: '/setmeal',
    method: 'delete',
    params: { ids }
  })
}

// 修改数据接口
export const editSetmeal = (params: MealPayload) => {
  return request({
    url: '/setmeal',
    method: 'put',
    data: { ...params }
  })
}

// 新增数据接口
export const addSetmeal = (params: MealPayload) => {
  return request({
    url: '/setmeal',
    method: 'post',
    data: { ...params }
  })
}

// 查询详情接口
export const querySetmealById = (id: string | (string | null)[]) => {
  return request({
    url: `/setmeal/${id}`,
    method: 'get'
  })
}

// 批量起售禁售
export const setmealStatusByStatus = (params: any) => {
  return request({
    url: `/setmeal/status/${params.status}`,
    method: 'post',
    params: { id: params.ids }
  })
}

//菜品分类数据查询
export const dishCategoryList = (params: any) => {
  return request({
    url: `/category/list`,
    method: 'get',
    params: { ...params }
  })
}
