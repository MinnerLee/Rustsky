const fs = require('node:fs')
const path = require('node:path')

const preview = path.resolve(__dirname, '../.preview')
const course = path.join(preview, 'course-miniprogram', 'mp-weixin')
const target = path.join(preview, 'miniprogram')
const [appId, endpoint] = process.argv.slice(2)
const production = appId !== undefined
const url = new URL(production ? endpoint : 'http://127.0.0.1:8888')
if (production && (!/^wx[a-f0-9]{16}$/i.test(appId) || url.protocol !== 'https:' ||
    url.pathname !== '/' || url.search || url.hash || url.username || url.password)) {
  throw new Error('Provide a WeChat App ID and an HTTPS origin.')
}
const output = production ? path.join(preview, 'miniprogram-wechat') : target
const origin = url.origin
const settings = production ? {} : readJson(path.join(preview, 'mysql', 'local-settings.json'))

if (!production && (typeof settings.customerDevCode !== 'string' || settings.customerDevCode.length < 16)) {
  throw new Error('Start the local Rustsky stack once before preparing the mini program.')
}

const app = readJson(path.join(course, 'app.json'))
for (const page of app.pages) {
  for (const extension of ['js', 'json', 'wxml', 'wxss']) {
    fs.accessSync(path.join(course, `${page}.${extension}`))
  }
}

let vendor = read(path.join(course, 'common', 'vendor.js'))
vendor = replace(vendor, "var baseUrl = 'http://localhost:8080';", `var baseUrl = '${origin}';`)
vendor = replace(vendor, '/user/addressBook/?id=', '/user/addressBook?id=')
vendor = replace(vendor, "resolve(res.data);", 'resolve(rustskyPreviewImages(res.data));')
if (!production) {
  vendor = `var rustskyLocal = require('../rustsky-local-config.js');
function rustskyLogin(options) {
  options.success({ errMsg: 'login:ok', code: rustskyLocal.devCode });
}
` + replace(vendor, 'uni.login({', 'rustskyLogin({', 2)
}
vendor = `function rustskyPreviewImages(value) {
  if (Array.isArray(value)) {
    value.forEach(rustskyPreviewImages);
  } else if (value && typeof value === 'object') {
    if (typeof value.image === 'string' && value.image.indexOf('/uploads/') === 0) {
      value.image = '${origin}' + value.image;
    }
    Object.keys(value).forEach(function (key) { rustskyPreviewImages(value[key]); });
  }
  return value;
}
` + vendor

let payment = read(path.join(course, 'pages', 'pay', 'index.js'))
payment = replace(payment, '      timeout: false,', '      timeout: false, paymentInProgress: false,')
payment = replace(payment, '    handleSave: function handleSave() {var _this = this;', `    handleSave: function handleSave() {var _this = this;
      if (this.paymentInProgress) return;`)
payment = replace(payment, '        // 如果支付成功进入成功页\n        clearTimeout(this.times);', '        this.paymentInProgress = true;')
payment = replace(payment, '            wx.requestPayment({', `            if (res.data && res.data.simulated) {
              clearTimeout(_this.times);
              wx.showToast({ title: '模拟付款成功', icon: 'success' });
              uni.redirectTo({ url: '/pages/success/index?orderId=' + _this.orderId });
              return;
            }
            wx.requestPayment({`)
payment = payment.replace(/success:function\(res\)\{\s*wx\.showModal\(\{[\s\S]*?console\.log\('支付成功!'\)\s*\}/, `complete: function () {
                _this.confirmRustskyPayment(params.orderNumber);
              }`)
if (!payment.includes('_this.confirmRustskyPayment(params.orderNumber)')) throw new Error('Missing checkout confirmation hook.')
payment = replace(payment, '    runTimeBack: function runTimeBack() {', `    confirmRustskyPayment: function (orderNumber) {
      var page = this;
      wx.request({
        url: '${origin}/user/order/payment/' + encodeURIComponent(orderNumber),
        header: { authentication: page.$store.state.token || '' },
        success: function (response) {
          if (response.data && response.data.code === 1 && response.data.data.payStatus === 1) {
            clearTimeout(page.times);
            uni.redirectTo({ url: '/pages/success/index?orderId=' + page.orderId });
          } else {
            page.paymentInProgress = false;
            wx.showToast({ title: '付款结果待确认，可在订单详情查看', icon: 'none' });
          }
        },
        fail: function () {
          page.paymentInProgress = false;
          wx.showToast({ title: '暂时无法确认付款结果', icon: 'none' });
        }
      });
    },
    runTimeBack: function runTimeBack() {`)
payment = replace(payment, "        });\n      }\n\n    },\n    // // 订单倒计时", `        }).catch(function (error) {
          _this.paymentInProgress = false;
          wx.showModal({ title: '提示', content: (error && error.msg) || '暂时无法付款', showCancel: false });
        });
      }

    },
    // // 订单倒计时`)

const config = readJson(path.join(course, 'project.config.json'))
config.appid = production ? appId : 'touristappid'
config.projectname = production ? 'Rustsky 微信小程序' : 'Rustsky 本地预览'
config.setting.urlCheck = production

fs.cpSync(course, output, { recursive: true })
fs.writeFileSync(path.join(output, 'common', 'vendor.js'), vendor)
fs.writeFileSync(path.join(output, 'pages', 'pay', 'index.js'), payment)
fs.writeFileSync(path.join(output, 'project.config.json'), JSON.stringify(config, null, 2) + '\n')
const privateConfig = path.join(output, 'project.private.config.json')
if (production) fs.rmSync(privateConfig, { force: true })
else fs.writeFileSync(path.join(output, 'rustsky-local-config.js'), 'module.exports = ' +
  JSON.stringify({ devCode: settings.customerDevCode }) + ';\n')
console.log('Prepared mini program at ' + output)

function read(file) {
  return fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n')
}

function readJson(file) {
  return JSON.parse(read(file).replace(/^\uFEFF/, ''))
}

function replace(text, original, replacement, expected = 1) {
  const count = text.split(original).length - 1
  if (count !== expected) {
    throw new Error(`Unexpected course bundle shape for ${JSON.stringify(original)}: ${count} match(es).`)
  }
  return text.split(original).join(replacement)
}
