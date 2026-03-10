use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct RequestLog {
    pub id: u64,
    pub timestamp: time::OffsetDateTime,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub body_preview: Option<String>,
    pub body_size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnStatus {
    Connected,
    Reconnecting { attempt: u32, next_retry_secs: u64 },
    Disconnected { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Normal,
    Detail,
    Filter,
    Help,
}

pub struct TuiState {
    pub requests: VecDeque<RequestLog>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub visible_rows: usize,
    pub filter_text: String,
    pub view_mode: ViewMode,
    pub show_qr: bool,
    pub prev_qr_state: bool,
    pub conn_status: ConnStatus,
    pub sparkline_data: VecDeque<u64>,
    pub total_requests: u64,
    pub total_duration_ms: u64,
    pub success_count: u64,
    pub tick_request_count: u64,
    pub max_requests: usize,
}

impl TuiState {
    pub fn new(max_requests: usize, visible_rows: usize) -> Self {
        Self {
            requests: VecDeque::with_capacity(max_requests),
            selected: 0,
            scroll_offset: 0,
            auto_scroll: true,
            visible_rows,
            filter_text: String::new(),
            view_mode: ViewMode::Normal,
            show_qr: true,
            prev_qr_state: true,
            conn_status: ConnStatus::Connected,
            sparkline_data: VecDeque::with_capacity(30),
            total_requests: 0,
            total_duration_ms: 0,
            success_count: 0,
            tick_request_count: 0,
            max_requests,
        }
    }

    pub fn push_request(&mut self, req: RequestLog) {
        if req.status >= 200 && req.status < 400 {
            self.success_count += 1;
        }
        self.total_requests += 1;
        self.total_duration_ms += req.duration_ms;
        self.tick_request_count += 1;
        self.requests.push_back(req);
        if self.requests.len() > self.max_requests {
            self.requests.pop_front();
            if !self.auto_scroll && self.selected > 0 {
                self.selected -= 1;
            }
            if !self.auto_scroll && self.scroll_offset > 0 {
                self.scroll_offset -= 1;
            }
        }
        if self.auto_scroll {
            let len = self.requests.len();
            self.selected = len.saturating_sub(1);
            self.scroll_offset = len.saturating_sub(self.visible_rows);
        }
    }

    pub fn tick(&mut self) {
        self.sparkline_data.push_back(self.tick_request_count);
        if self.sparkline_data.len() > 30 {
            self.sparkline_data.pop_front();
        }
        self.tick_request_count = 0;
    }

    pub fn filtered_requests(&self) -> Vec<(usize, &RequestLog)> {
        if self.filter_text.is_empty() {
            return self.requests.iter().enumerate().collect();
        }
        let f = self.filter_text.to_lowercase();
        self.requests
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.path.to_lowercase().contains(&f)
                    || r.method.to_lowercase().contains(&f)
                    || r.status.to_string().contains(&f)
            })
            .collect()
    }

    pub fn visible_requests(&self) -> Vec<(usize, &RequestLog, bool)> {
        let filtered = self.filtered_requests();
        let end = (self.scroll_offset + self.visible_rows).min(filtered.len());
        let start = self.scroll_offset.min(end);
        filtered[start..end]
            .iter()
            .enumerate()
            .map(|(vis_idx, (orig_idx, req))| {
                let is_selected = *orig_idx == self.selected || (start + vis_idx) == self.selected;
                (*orig_idx, *req, is_selected)
            })
            .collect()
    }

    pub fn avg_duration_ms(&self) -> u64 {
        if self.total_requests == 0 {
            0
        } else {
            self.total_duration_ms / self.total_requests
        }
    }

    pub fn success_rate(&self) -> u64 {
        if self.total_requests == 0 {
            100
        } else {
            self.success_count * 100 / self.total_requests
        }
    }

    pub fn clear(&mut self) {
        self.requests.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.sparkline_data.clear();
        self.total_requests = 0;
        self.total_duration_ms = 0;
        self.success_count = 0;
    }

    pub fn select_up(&mut self) {
        self.auto_scroll = false;
        let filtered = self.filtered_requests();
        if filtered.is_empty() {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    pub fn select_down(&mut self) {
        let filtered = self.filtered_requests();
        if filtered.is_empty() {
            return;
        }
        let max = filtered.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        if self.selected >= self.scroll_offset + self.visible_rows {
            self.scroll_offset = self.selected + 1 - self.visible_rows;
        }
        if self.selected >= max {
            self.auto_scroll = true;
        }
    }
}
