
pub enum EmailTemplate {
    OrderPlaced {
        order_key:        String,
        guest_email:      String,
        total_amount_gbp: String,
        product_name:     String,
    },
    OrderStatusUpdated {
        order_key:    String,
        guest_email:  String,
        new_status:   String,
        product_name: String,
    },
    FarmerNewOrder {
        order_key:        String,
        customer_email:   String,
        product_name:     String,
        total_amount_gbp: String,
    },
    FarmerLowStock {
        product_name:       String,
        quantity_available: String,
        threshold:          String,
    },
    VerifyEmail {
        farm_name:  String,
        verify_url: String,
    },
}

// ── Methods ────────────────────────────────────────────────────────────────────

impl EmailTemplate {
    pub fn subject(&self) -> String {
        match self {
            Self::OrderPlaced        { order_key, .. }             => format!("Order confirmed — #{order_key}"),
            Self::OrderStatusUpdated { new_status, order_key, .. } => format!("Your order #{order_key} is now {new_status}"),
            Self::FarmerNewOrder     { order_key, .. }             => format!("New order received — #{order_key}"),
            Self::FarmerLowStock     { product_name, .. }          => format!("Low stock alert — {product_name}"),
            Self::VerifyEmail        { farm_name, .. }             => format!("Verify your AgriMarket account — {farm_name}"),
        }
    }

    pub fn html_body(&self) -> String {
        match self {
            Self::OrderPlaced { order_key, total_amount_gbp, product_name, .. } =>
                order_placed_html(order_key, product_name, total_amount_gbp),
            Self::OrderStatusUpdated { order_key, new_status, product_name, .. } =>
                status_update_html(order_key, new_status, product_name),
            Self::FarmerNewOrder { order_key, customer_email, product_name, total_amount_gbp } =>
                farmer_new_order_html(order_key, customer_email, product_name, total_amount_gbp),
            Self::FarmerLowStock { product_name, quantity_available, threshold } =>
                farmer_low_stock_html(product_name, quantity_available, threshold),
            Self::VerifyEmail { farm_name, verify_url } =>
                verify_email_html(farm_name, verify_url),
        }
    }
}

// ── HTML template functions ────────────────────────────────────────────────────

fn base_html(content: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
</head>
<body style="margin:0;padding:0;background:#f5f0e8;font-family:'Helvetica Neue',Arial,sans-serif;">
  <table width="100%" cellpadding="0" cellspacing="0">
    <tr><td align="center" style="padding:40px 16px;">
      <table width="560" cellpadding="0" cellspacing="0"
        style="background:#fff;border-radius:16px;overflow:hidden;box-shadow:0 2px 12px rgba(0,0,0,0.08);">
        <tr>
          <td style="background:linear-gradient(135deg,#1a3a2e,#254d3a);padding:28px 36px;">
            <span style="color:#fff;font-size:20px;font-weight:700;">🌱 AgriMarket</span>
          </td>
        </tr>
        <tr><td style="padding:32px 36px;color:#1f2937;font-size:15px;line-height:1.6;">
          {content}
        </td></tr>
        <tr>
          <td style="padding:20px 36px;background:#f9f5ee;border-top:1px solid #e8e0d0;
            font-size:12px;color:#9ca3af;text-align:center;">
            AgriMarket · Farm-fresh food, direct to your door<br>
            <span style="color:#d1cfc9;">You're receiving this because you placed an order with us.</span>
          </td>
        </tr>
      </table>
    </td></tr>
  </table>
</body>
</html>"#, content = content)
}

fn order_placed_html(order_key: &str, product: &str, total: &str) -> String {
    let content = format!(r#"
<h2 style="margin:0 0 16px;color:#1a3a2e;font-size:22px;">Order Confirmed ✓</h2>
<p>Thank you for your order. Here's a summary:</p>
<table width="100%" style="margin:20px 0;border-radius:10px;overflow:hidden;border:1px solid #e8e0d0;">
  <tr style="background:#f5f0e8;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Order reference</td>
    <td style="padding:12px 16px;font-weight:600;color:#1a3a2e;">#{order_key}</td>
  </tr>
  <tr>
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Product</td>
    <td style="padding:12px 16px;font-weight:600;">{product}</td>
  </tr>
  <tr style="background:#f5f0e8;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Total</td>
    <td style="padding:12px 16px;font-weight:700;color:#e07b28;">£{total}</td>
  </tr>
</table>
<p style="color:#6b7280;font-size:14px;">
  Your farmer has been notified and will confirm your order shortly.
  You'll receive another email when your order status changes.
</p>
<p style="margin-top:24px;font-size:14px;color:#6b7280;">
  Questions? Reply to this email and we'll help you out.
</p>"#);
    base_html(&content)
}

fn status_update_html(order_key: &str, status: &str, product: &str) -> String {
    let (icon, message) = match status {
        "confirmed"  => ("✅", "Your order has been confirmed by the farmer."),
        "processing" => ("⚙️",  "Your order is being prepared."),
        "dispatched" => ("🚚", "Your order is on its way!"),
        "delivered"  => ("🎉", "Your order has been delivered. Enjoy your fresh produce!"),
        "cancelled"  => ("❌", "Your order has been cancelled. Contact us if you have questions."),
        _            => ("📦", "Your order status has been updated."),
    };
    let status_cap = format!("{}{}", &status[..1].to_uppercase(), &status[1..]);
    let content = format!(r#"
<h2 style="margin:0 0 16px;color:#1a3a2e;font-size:22px;">{icon} Order {status_cap}</h2>
<p>{message}</p>
<table width="100%" style="margin:20px 0;border-radius:10px;overflow:hidden;border:1px solid #e8e0d0;">
  <tr style="background:#f5f0e8;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Order reference</td>
    <td style="padding:12px 16px;font-weight:600;color:#1a3a2e;">#{order_key}</td>
  </tr>
  <tr>
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Product</td>
    <td style="padding:12px 16px;font-weight:600;">{product}</td>
  </tr>
  <tr style="background:#f5f0e8;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Status</td>
    <td style="padding:12px 16px;">
      <span style="background:#dcfce7;color:#166534;padding:3px 10px;
        border-radius:20px;font-size:13px;font-weight:600;">
        {status_cap}
      </span>
    </td>
  </tr>
</table>"#);
    base_html(&content)
}

fn farmer_new_order_html(order_key: &str, customer: &str, product: &str, total: &str) -> String {
    let content = format!(r#"
<h2 style="margin:0 0 16px;color:#1a3a2e;font-size:22px;">New Order Received 🛒</h2>
<p>A customer has placed an order for your product.</p>
<table width="100%" style="margin:20px 0;border-radius:10px;overflow:hidden;border:1px solid #e8e0d0;">
  <tr style="background:#f5f0e8;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Order reference</td>
    <td style="padding:12px 16px;font-weight:600;color:#1a3a2e;">#{order_key}</td>
  </tr>
  <tr>
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Customer</td>
    <td style="padding:12px 16px;">{customer}</td>
  </tr>
  <tr style="background:#f5f0e8;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Product</td>
    <td style="padding:12px 16px;font-weight:600;">{product}</td>
  </tr>
  <tr>
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Revenue</td>
    <td style="padding:12px 16px;font-weight:700;color:#e07b28;">£{total}</td>
  </tr>
</table>
<p style="font-size:14px;color:#6b7280;">
  Log in to your AgriMarket dashboard to confirm and process this order.
</p>"#);
    base_html(&content)
}

fn farmer_low_stock_html(product: &str, available: &str, threshold: &str) -> String {
    let content = format!(r#"
<h2 style="margin:0 0 16px;color:#b45309;font-size:22px;">⚠️ Low Stock Alert</h2>
<p>One of your products is running low and may need restocking soon.</p>
<table width="100%" style="margin:20px 0;border-radius:10px;overflow:hidden;border:1px solid #fde68a;">
  <tr style="background:#fef9c3;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Product</td>
    <td style="padding:12px 16px;font-weight:600;">{product}</td>
  </tr>
  <tr>
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Available</td>
    <td style="padding:12px 16px;font-weight:700;color:#dc2626;">{available}</td>
  </tr>
  <tr style="background:#fef9c3;">
    <td style="padding:12px 16px;font-size:13px;color:#6b7280;">Your threshold</td>
    <td style="padding:12px 16px;">{threshold}</td>
  </tr>
</table>
<p style="font-size:14px;color:#6b7280;">
  Visit your stock management page to restock this product before it runs out.
</p>"#);
    base_html(&content)
}

fn verify_email_html(farm_name: &str, verify_url: &str) -> String {
    let content = format!(r#"
<h2 style="margin:0 0 16px;color:#1a3a2e;font-size:22px;">Welcome to AgriMarket 🌱</h2>
<p>Hi <strong>{farm_name}</strong>, thanks for joining AgriMarket.</p>
<p style="color:#6b7280;font-size:14px;">
  Please verify your email address to activate your account
  and start listing your products.
</p>
<div style="text-align:center;margin:32px 0;">
  <a href="{verify_url}"
    style="background:linear-gradient(135deg,#1a3a2e,#254d3a);
           color:#fff;text-decoration:none;padding:14px 32px;
           border-radius:10px;font-weight:600;font-size:15px;
           display:inline-block;">
    Verify my account →
  </a>
</div>
<p style="color:#9ca3af;font-size:13px;text-align:center;">
  This link expires in 24 hours.<br>
  If you didn't create an account, you can safely ignore this email.
</p>"#);
    base_html(&content)
}