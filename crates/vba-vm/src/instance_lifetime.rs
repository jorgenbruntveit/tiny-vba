//! Explicit references outside the canonical value store.
use super::*;

impl Session {
    pub(super) fn retain_operand(&mut self, operand: &Operand) {
        match operand {
            Operand::Value(value) => self.store.object_references().retain_value(value),
            Operand::Reference(location) => {
                if let Some(owner) = self.store.object_owner(*location) {
                    self.store.object_references().retain(owner);
                }
            }
        }
    }

    pub(super) fn release_operand(&mut self, operand: &Operand) {
        match operand {
            Operand::Value(value) => self.store.object_references().release_value(value),
            Operand::Reference(location) => {
                if let Some(owner) = self.store.object_owner(*location) {
                    self.store.object_references().release(owner);
                }
            }
        }
    }

    pub(super) fn retain_delivery(&mut self, delivery: &classes::Delivery) {
        self.store.object_references().retain(delivery.receiver);
        for argument in &delivery.arguments {
            self.retain_operand(argument);
        }
    }

    pub(super) fn release_delivery(&mut self, delivery: &classes::Delivery) {
        self.store.object_references().release(delivery.receiver);
        for argument in &delivery.arguments {
            self.release_operand(argument);
        }
    }

    pub(super) fn clear_operand_stack(&mut self) -> Result<()> {
        let mut stack = std::mem::take(&mut self.top_mut()?.stack);
        for operand in stack.drain(..) {
            self.release_operand(&operand);
        }
        self.top_mut()?.stack = stack;
        Ok(())
    }

    pub(super) fn clear_event_deliveries(&mut self) -> Result<()> {
        let events = std::mem::take(&mut self.top_mut()?.events);
        for delivery in &events {
            self.release_delivery(delivery);
        }
        Ok(())
    }

    pub(super) fn clear_captured_references(&mut self) -> Result<()> {
        for location in std::mem::take(&mut self.top_mut()?.captured).into_values() {
            self.release_operand(&Operand::Reference(location));
        }
        Ok(())
    }

    pub(super) fn release_frame_references(&mut self, frame: &Frame) {
        if let Some(ClassReturn::AutoInitialize { location, .. }) = frame.class_return {
            self.release_operand(&Operand::Reference(location));
        }
        for location in frame.captured.values() {
            self.release_operand(&Operand::Reference(*location));
        }
        for operand in &frame.stack {
            self.release_operand(operand);
        }
        for delivery in &frame.events {
            self.release_delivery(delivery);
        }
        let references = self.store.object_references();
        for object in frame
            .receiver
            .into_iter()
            .chain(frame.borrowed_owners.iter().copied())
            .chain(frame.enumerator_owners.iter().copied())
        {
            references.release(object);
        }
        for cursor in frame.enumerations.values() {
            references.release_array(&cursor.values);
        }
    }

    pub(super) fn clear_enumerator_owners(&mut self) -> Result<()> {
        for object in std::mem::take(&mut self.top_mut()?.enumerator_owners) {
            self.store.object_references().release(object);
        }
        Ok(())
    }

    pub(super) fn expire_enumerations(&mut self, span: vba_diagnostics::Span) -> Result<()> {
        let frame = self
            .frames
            .last_mut()
            .ok_or_else(|| invariant("no frame"))?;
        if frame.handler.fault.is_none() {
            let references = self.store.object_references();
            frame.enumerations.retain(|_, cursor| {
                let within_loop = span.start() >= cursor.start && span.end() <= cursor.end;
                if !within_loop {
                    references.release_array(&cursor.values);
                }
                within_loop
            });
        }
        self.clear_enumerator_owners()
    }

    pub(super) fn take_pending_result(&mut self) -> Option<ReturnValue> {
        let result = self.pending_result.take();
        if let Some(ReturnValue::Function(value)) = &result {
            self.store.object_references().release_value(value);
        }
        result
    }

    pub(super) fn set_pending_result(&mut self, result: ReturnValue) {
        if let ReturnValue::Function(value) = &result {
            self.store.object_references().retain_value(value);
        }
        self.pending_result = Some(result);
    }

    pub(super) fn remember_default_instance(
        &mut self,
        source: ir::SourceIndex,
        object: vba_values::ObjectRef,
    ) {
        let references = self.store.object_references();
        references.retain(object);
        if let Some(previous) = self.default_instances.insert(source, object) {
            references.release(previous);
        }
    }

    pub(super) fn forget_default_instance(&mut self, source: ir::SourceIndex) {
        if let Some(object) = self.default_instances.remove(&source) {
            self.store.object_references().release(object);
        }
    }

    pub(super) fn forget_instances(&mut self) {
        for instance in self.instances.drain(..) {
            self.store.object_references().unregister(instance.object);
            self.instance_base = instance.object.object;
        }
    }

    pub(super) fn release_host_argument_owners(&mut self) {
        let references = self.store.object_references();
        for owner in self.host_argument_owners.drain(..) {
            references.release(owner);
        }
    }

    pub(super) fn release_callback_references(&mut self, callback: &ScheduledCallback) {
        let references = self.store.object_references();
        if let Some(object) = callback.receiver {
            references.release(object);
        }
        for value in &callback.arguments {
            references.release_value(value);
        }
    }
}
