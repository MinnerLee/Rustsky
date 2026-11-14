use axum::Router;
use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};
use utoipa_swagger_ui::{Config, SwaggerUi};

#[derive(OpenApi)]
#[openapi(
    info(title = "Rustsky 接口文档", description = "通过本地 Nginx 访问 Rustsky 的管理端和用户端接口。"),
    servers((url = "/", description = "当前 Nginx 入口")),
    paths(crate::health, crate::employee::login, crate::employee::logout, crate::employee::create::create_employee, crate::employee::page::list_employees,
          crate::employee::manage::employee_details, crate::employee::manage::edit_employee, crate::employee::manage::set_employee_status,
          crate::category::page, crate::category::list, crate::category::create, crate::category::edit, crate::category::set_status, crate::category::remove,
          crate::dish::page, crate::dish::details, crate::dish::create, crate::dish::edit, crate::dish::remove, crate::dish::list, crate::dish::set_status, crate::upload::upload,
          crate::setmeal::page, crate::setmeal::details, crate::setmeal::create, crate::setmeal::edit, crate::setmeal::remove, crate::setmeal::set_status,
          crate::shop::admin_status, crate::shop::user_status, crate::shop::set_status,
          crate::menu::categories, crate::menu::dishes, crate::menu::setmeals, crate::menu::meal_dishes,
          crate::customer::login, crate::address::list, crate::address::details, crate::address::default_address,
          crate::address::create, crate::address::edit, crate::address::set_default, crate::address::remove,
          crate::cart::add, crate::cart::subtract, crate::cart::list, crate::cart::clean, crate::order::submit,
          crate::order::query::admin_page, crate::order::query::admin_details, crate::order::query::counts,
          crate::order::query::history, crate::order::query::customer_details, crate::order::manage::confirm,
          crate::order::manage::delivery, crate::order::manage::complete, crate::order::manage::admin_cancel,
          crate::order::manage::reject, crate::order::manage::customer_cancel, crate::order::manage::repeat,
          crate::order::manage::simulate_payment, crate::order::manage::miniprogram_payment, crate::order::wechat::notification, crate::order::wechat::status,
          crate::order::automation::remind,
          crate::report::turnover, crate::report::users, crate::report::orders, crate::report::top,
          crate::report::overview, crate::report::export, crate::report::business,
          crate::report::workspace_orders, crate::report::workspace_dishes, crate::report::workspace_setmeals),
    tags((name = "员工", description = "管理端员工和登录"), (name = "分类", description = "菜品和套餐分类"),
         (name = "菜品", description = "菜品、口味和图片"), (name = "套餐", description = "套餐和关联菜品"), (name = "店铺", description = "营业状态"),
         (name = "菜单", description = "用户端浏览菜单"), (name = "用户", description = "小程序用户登录"),
         (name = "地址簿", description = "用户收货地址"), (name = "购物车", description = "用户购物车"),
         (name = "订单", description = "用户下单和订单管理"), (name = "统计", description = "工作台、营业统计和 Excel 导出"),
         (name = "运行状态", description = "本地 HTTP 健康检查")),
    modifiers(&TokenAuth)
)]
struct ApiDoc;

struct TokenAuth;

impl Modify for TokenAuth {
    fn modify(&self, api: &mut utoipa::openapi::OpenApi) {
        api.components.get_or_insert_default().add_security_scheme(
            "EmployeeToken",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                "token",
                "填写登录响应中的 data.token，直接粘贴 JWT 字符串。",
            ))),
        );
        api.components.get_or_insert_default().add_security_scheme(
            "CustomerToken",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                "authentication",
                "填写用户登录响应中的 data.token。",
            ))),
        );
    }
}

pub fn routes() -> Router {
    SwaggerUi::new("/docs")
        .url("/openapi.json", ApiDoc::openapi())
        .config(Config::default().validator_url("none"))
        .into()
}

pub fn export() -> Result<String, Box<dyn std::error::Error>> {
    Ok(ApiDoc::openapi().to_pretty_json()?)
}
