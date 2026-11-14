<template>
  <div class="addDish">
    <div class="leftCont">
      <div v-show="seachKey.trim() == ''"
           class="tabBut">
        <span v-for="(item, index) in dishType"
              :key="index"
              :class="{ act: index == keyInd }"
              @click="checkTypeHandle(index)">{{ item.name }}</span>
      </div>
      <div class="tabList">
        <div class="table"
             :class="{ borderNone: !dishList.length }">
          <div v-if="dishList.length == 0"
               style="padding-left: 10px">
            <Empty />
          </div>
          <el-checkbox-group v-if="dishList.length > 0"
                             v-model="checkedList"
                             @change="checkedListHandle">
            <div v-for="item in dishList"
                 :key="item.dishId"
                 class="items">
              <el-checkbox :label="item.dishId">
                <div class="item">
                  <span style="flex: 3; text-align: left">{{
                    item.name
                  }}</span>
                  <span>{{ item.status == 0 ? '停售' : '在售' }}</span>
                  <span>{{ (Number(item.price) ).toFixed(2)*100/100 }}</span>
                </div>
              </el-checkbox>
            </div>
          </el-checkbox-group>
        </div>
      </div>
    </div>
    <div class="ritCont">
      <div class="tit">
        已选菜品({{ checkedListAll.length }})
      </div>
      <div class="items">
        <div v-for="item in checkedListAll"
             :key="item.dishId"
             class="item">
          <span>{{ item.name }}</span>
          <span class="price">￥ {{ (Number(item.price) ).toFixed(2)*100/100 }} </span>
          <span class="del"
                @click="delCheck(item.dishId)">
            <img src="./../../../assets/icons/btn_clean@2x.png"
                 alt="">
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<script lang="ts">
import { Component, Prop, Vue, Watch } from 'vue-property-decorator'
import { getCategoryList, queryDishList } from '@/api/dish'
import Empty from '@/components/Empty/index.vue'
import { MealDish, MealCategory } from '@/api/setMeal'

@Component({ name: 'SetmealDishPicker', components: { Empty } })
export default class extends Vue {
  @Prop({ default: () => [] }) private checkList!: MealDish[]
  @Prop({ default: '' }) private seachKey!: string

  private dishType: MealCategory[] = []
  private dishList: MealDish[] = []
  private checkedList: number[] = []
  private checkedListAll: MealDish[] = []
  private keyInd = 0
  private requestSequence = 0

  async created() {
    this.checkedListAll = this.checkList.map(dish => ({ ...dish }))
    this.checkedList = this.checkedListAll.map(dish => dish.dishId)
    try {
      const { data } = await getCategoryList({ type: 1 })
      if (data.code !== 1) return this.$message.error(data.msg || '无法查询菜品分类')
      this.dishType = data.data
      this.loadDishes()
    } catch (error) {
      this.$message.error('无法查询菜品分类')
    }
  }

  @Watch('seachKey')
  private async loadDishes() {
    const request = ++this.requestSequence
    const name = this.seachKey.trim()
    const category = this.dishType[this.keyInd]
    if (!name && !category) {
      this.dishList = []
      return
    }
    try {
      const { data } = await queryDishList(name ? { name } : { categoryId: category.id })
      if (request !== this.requestSequence) return
      if (data.code !== 1) return this.$message.error(data.msg || '无法查询菜品')
      this.dishList = data.data.map(dish => ({ ...dish, dishId: dish.id, copies: 1 }))
    } catch (error) {
      if (request === this.requestSequence) this.$message.error('无法查询菜品')
    }
  }

  private checkTypeHandle(index: number) {
    this.keyInd = index
    this.loadDishes()
  }

  private checkedListHandle(ids: number[]) {
    const previous = new Map(this.checkedListAll.map(dish => [dish.dishId, dish] as const))
    const visible = new Map(this.dishList.map(dish => [dish.dishId, dish] as const))
    this.checkedListAll = ids.reduce((selection, id) => {
      const dish = previous.get(id) || visible.get(id)
      if (dish) selection.push(dish)
      return selection
    }, [] as MealDish[])
    this.$emit('checkList', this.checkedListAll)
  }

  private delCheck(id: number) {
    this.checkedList = this.checkedList.filter(dishId => dishId !== id)
    this.checkedListHandle(this.checkedList)
  }
}
</script>
<style lang="scss">
.addDish {
  .el-checkbox__label {
    width: 100%;
  }
  .empty-box {
    margin-top: 50px;
    margin-bottom: 0px;
  }
}
</style>
<style lang="scss" scoped>
.addDish {
  padding: 0 20px;
  display: flex;
  line-height: 40px;
  .empty-box {
    img {
      width: 190px;
      height: 147px;
    }
  }

  .borderNone {
    border: none !important;
  }
  span,
  .tit {
    color: #333;
  }
  .leftCont {
    display: flex;
    border-right: solid 1px #efefef;
    width: 60%;
    padding: 15px;
    .tabBut {
      width: 110px;
      font-weight: bold;
      border-right: solid 2px #f4f4f4;
      span {
        display: block;
        text-align: center;
        // border-right: solid 2px #f4f4f4;
        cursor: pointer;
        position: relative;
      }
    }
    .act {
      border-color: $mine !important;
      color: $mine !important;
    }
    .act::after {
      content: ' ';
      display: inline-block;
      background-color: $mine;
      width: 2px;
      height: 40px;
      position: absolute;
      right: -2px;
    }
    .tabList {
      flex: 1;
      padding: 15px;
      height: 400px;
      overflow-y: scroll;
      .table {
        border: solid 1px #f4f4f4;
        border-bottom: solid 1px #f4f4f4;
        .items {
          border-bottom: solid 1px #f4f4f4;
          padding: 0 10px;
          display: flex;
          .el-checkbox,
          .el-checkbox__label {
            width: 100%;
          }
          .item {
            display: flex;
            padding-right: 20px;
            span {
              display: inline-block;
              text-align: center;
              flex: 1;
              font-weight: normal;
            }
          }
        }
      }
    }
  }
  .ritCont {
    width: 40%;
    .tit {
      margin: 0 15px;
      font-weight: bold;
    }
    .items {
      height: 338px;
      padding: 4px 15px;
      overflow: scroll;
    }
    .item {
      box-shadow: 0px 1px 4px 3px rgba(0, 0, 0, 0.03);
      display: flex;
      text-align: center;
      padding: 0 10px;
      margin-bottom: 20px;
      border-radius: 6px;
      color: #818693;
      span:first-child {
        text-align: left;
        color: #20232a;
        flex: 70%;
      }
      .price {
        display: inline-block;
        flex: 70%;
        text-align: left;
      }
      .del {
        cursor: pointer;
        img {
          position: relative;
          top: 5px;
          width: 20px;
        }
      }
    }
  }
}
</style>
