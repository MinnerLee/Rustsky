const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { execFileSync } = require('node:child_process')
const { randomBytes } = require('node:crypto')
const root = path.resolve(__dirname, '..')
const { chromium } = require(process.env.RUSTSKY_PLAYWRIGHT_MODULE || path.join(root, '.preview/browser/node_modules/playwright-core'))
const runtime = path.join(root, '.preview/mysql')
const settings = JSON.parse(fs.readFileSync(path.join(runtime, 'local-settings.json'), 'utf8').replace(/^\uFEFF/, ''))
const origin = 'http://127.0.0.1:8888'
const nonce = randomBytes(4).toString('hex')
const username = 'ui' + nonce
const employeeNo = 'UI-' + nonce.toUpperCase()
const mealName = '套餐' + nonce
const mealEdited = mealName + '改'
const categories = ['菜甲', '菜乙', '套类'].map(label => label + nonce)
const dishNames = ['甲菜' + nonce, '乙菜' + nonce]
const sql = statement => execFileSync(path.join(runtime, 'mysql-8.4.13-winx64/bin/mysql.exe'), [
  '--defaults-extra-file=' + path.join(runtime, 'root-client.ini'),
  '--database=rustsky_dev', '--execute=' + statement
], { stdio: ['ignore', 'ignore', 'pipe'] })

async function verify() {
  const executablePath = process.env.RUSTSKY_CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe'
  const browser = await chromium.launch({ executablePath, headless: true })
  const page = await browser.newPage({ viewport: { width: 1540, height: 950 }, serviceWorkers: 'block' })
  const photo = path.join(root, 'frontend/src/assets/login/hot-pot.jpg')
  const issues = []
  page.on('pageerror', error => issues.push(error.message))
  const clicked = (method, pathname, action) => Promise.all([
    page.waitForResponse(response => new URL(response.url()).pathname === '/api' + pathname && response.request().method() === method, { timeout: 8000 }),
    action()
  ]).then(async ([response]) => {
    const body = await response.json()
    assert.equal(body.code, 1, body.msg)
    return body.data
  })
  const fill = (label, value) => page.getByPlaceholder(label, { exact: true }).fill(value)
  const button = (name, container = page) => container.getByRole('button', { name, exact: true })
  const row = text => page.locator('.el-table__body-wrapper tr').filter({ hasText: text }).first()
  let headers = {}
  let createdImages = new Set()
  try {
    console.log('Browser login and employee editor')
    await page.goto(origin + '/sky/#/login')
    await fill('账号', settings.employeeUsername)
    await fill('密码', settings.employeePassword)
    const user = await clicked('POST', '/employee/login', () => button('登录').click())
    headers = { token: user.token }
    await page.waitForURL(url => !url.hash.startsWith('#/login'))
    await page.goto(origin + '/sky/#/employee')
    await button('+ 添加员工').click()
    await fill('请输入账号', username)
    await fill('请输入员工姓名', '浏览器员工')
    await fill('例如 RS-0007', employeeNo)
    await fill('请输入 8 至 64 位初始密码', randomBytes(12).toString('hex'))
    await fill('请输入手机号', '13900000000')
    await fill('请输入身份证号', '000000000000000000')
    await clicked('POST', '/employee', () => button('保存').click())
    await page.waitForURL(url => url.hash === '#/employee')
    await fill('请输入员工姓名', '浏览器员工')
    await clicked('GET', '/employee/page', () => button('查询').click())
    await row(username).getByText(employeeNo, { exact: true }).waitFor()
    await button('修改', row(username)).click()
    await page.getByPlaceholder('例如 RS-0007').waitFor()
    await page.waitForFunction(expected => document.querySelector('input[placeholder="例如 RS-0007"]').value === expected, employeeNo)
    await fill('请输入员工姓名', '浏览器修改')
    await fill('例如 RS-0007', employeeNo + '-00')
    await clicked('PUT', '/employee', () => button('保存').click())
    await page.waitForURL(url => url.hash === '#/employee')
    await row(username).getByText(employeeNo + '-00', { exact: true }).waitFor()
    for (const [label, status] of [['禁用', 0], ['启用', 1]]) {
      await button(label, row(username)).click()
      await clicked('POST', '/employee/status/' + status, () => button('确定', page.locator('.el-message-box')).click())
    }

    console.log('Browser setmeal search, edit, status and deletion')
    const call = async (method, route, data) => {
      const response = await page.request.fetch(origin + '/api' + route, { method, headers, data })
      const body = await response.json()
      assert.equal(body.code, 1, body.msg)
      return body.data
    }
    const categoryIds = []
    for (const [index, name] of categories.entries()) {
      const type = index === 2 ? 2 : 1
      await call('POST', '/category', { name, type, sort: 1 })
      const found = (await call('GET', '/category/page?name=' + encodeURIComponent(name))).records[0]
      assert.ok(found)
      await call('POST', '/category/status/1?id=' + found.id)
      categoryIds.push(found.id)
    }
    for (const [index, name] of dishNames.entries()) {
      await call('POST', '/dish', { name, categoryId: categoryIds[index], price: '20.25', image: '/uploads/test-fixture.jpg', description: '', flavors: [] })
      const result = await call('GET', '/dish/page?name=' + encodeURIComponent(name))
      await call('POST', '/dish/status/1?id=' + result.records[0].id)
    }
    await page.goto(origin + '/sky/#/setmeal')
    await button('+ 新建套餐').click()
    await page.waitForURL(url => url.hash === '#/setmeal/add')
    await page.waitForFunction(() => {
      const input = document.querySelector('input[placeholder="请填写套餐名称"]')
      let node = input
      while (node) {
        if (node.__vue__ && node.__vue__.$options.name === 'SetmealForm') {
          return !node.__vue__.loading && node.__vue__.setMealList.length > 0
        }
        node = node.parentElement
      }
      return false
    })
    await fill('请填写套餐名称', mealName)
    assert.equal(await page.getByPlaceholder('请填写套餐名称').inputValue(), mealName, 'name after fill')
    await page.getByPlaceholder('请选择套餐分类').click()
    await page.locator('.el-select-dropdown:visible').getByText(categories[2], { exact: true }).click()
    assert.equal(await page.getByPlaceholder('请填写套餐名称').inputValue(), mealName, 'name after selecting category')
    await fill('请设置套餐价格', '45.50')
    assert.equal(await page.getByPlaceholder('请设置套餐价格').inputValue(), '45.50', 'price after fill')
    const image = await clicked('POST', '/common/upload', () => page.locator('input[type=file]').setInputFiles(photo))
    createdImages.add(image)
    assert.equal(await page.getByPlaceholder('请填写套餐名称').inputValue(), mealName, 'name after upload')
    await page.getByText('+ 添加菜品', { exact: true }).click()
    const dialog = page.locator('.el-dialog:visible')
    const searchDish = async name => {
      const input = dialog.getByPlaceholder('请输入菜品名称进行搜索')
      await input.fill(name)
      await clicked('GET', '/dish/list', () => input.press('Enter'))
    }
    await searchDish(dishNames[0])
    await dialog.locator('label.el-checkbox').filter({ hasText: dishNames[0] }).click()
    await searchDish(dishNames[1])
    await dialog.locator('label.el-checkbox').filter({ hasText: dishNames[1] }).click()
    await button('添 加', dialog).click()
    await row(dishNames[0]).locator('.el-input-number input').fill('3')
    await row(dishNames[0]).locator('.el-input-number input').press('Tab')
    assert.equal(await page.getByPlaceholder('请填写套餐名称').inputValue(), mealName)
    assert.equal(await page.getByPlaceholder('请设置套餐价格').inputValue(), '45.50')
    await clicked('POST', '/setmeal', () => button('保存').click())
    await page.waitForURL(url => url.hash === '#/setmeal')
    await fill('请填写套餐名称', mealName)
    await clicked('GET', '/setmeal/page', () => button('查询').click())
    await button('修改', row(mealName)).click()
    await page.waitForFunction(name => document.querySelector('input[placeholder="请填写套餐名称"]').value === name, mealName)
    await page.getByText('+ 添加菜品', { exact: true }).click()
    await button('添 加', dialog).click()
    assert.equal(await row(dishNames[0]).locator('.el-input-number input').inputValue(), '3')
    await fill('请填写套餐名称', mealEdited)
    await clicked('PUT', '/setmeal', () => button('保存').click())
    await page.waitForURL(url => url.hash === '#/setmeal')
    await row(mealEdited).waitFor()
    assert.equal((await call('GET', '/setmeal/page?name=' + encodeURIComponent(mealEdited))).records[0].price, 45.5)
    for (const [label, status] of [['启售', 1], ['停售', 0]]) {
      await button(label, row(mealEdited)).click()
      await clicked('POST', '/setmeal/status/' + status, () => button('确定', page.locator('.el-message-box')).click())
    }
    await row(mealEdited).locator('label.el-checkbox').click()
    await page.getByText('批量删除', { exact: true }).click()
    await clicked('DELETE', '/setmeal', () => button('删除', page.locator('.el-message-box')).click())
    await row(mealEdited).waitFor({ state: 'detached' })
    assert.deepEqual(issues, [])
    console.log('PASS browser login, employee create/edit/search/enable/disable, setmeal image/dish search/create/edit/status/batch deletion')
  } catch (error) {
    const messages = await page.locator('.el-form-item__error, .el-message__content').allTextContents()
    if (messages.length) console.error('Visible form and status messages:', messages)
    if (issues.length) console.error('Browser errors:', issues)
    throw error
  } finally {
    try {
      await browser.close()
      sql(`DELETE sd FROM setmeal_dish sd JOIN setmeal s ON s.id=sd.setmeal_id WHERE s.name IN ('${mealName}','${mealEdited}');
      DELETE FROM setmeal WHERE name IN ('${mealName}','${mealEdited}');
      DELETE FROM dish WHERE name IN ('${dishNames.join("','")}');
      DELETE FROM category WHERE name IN ('${categories.join("','")}');
      DELETE FROM employee WHERE username='${username}' AND employee_no IN ('${employeeNo}','${employeeNo}-00');`)
    } finally {
      for (const image of createdImages) {
        if (/^\/uploads\/[a-f0-9]{32}\.jpg$/.test(image)) fs.rmSync(path.join(root, '.preview', image), { force: true })
      }
    }
  }
}

verify().catch(error => {
  console.error(error)
  process.exitCode = 1
})
