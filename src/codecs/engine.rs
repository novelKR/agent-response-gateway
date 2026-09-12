//! The reference process uses the same codec library as the built-in execution path.
use super::contract::*;
use crate::{
    adapters::{PreparedAdapter, managed::ManagedAdapter, sse::SseEvent},
    ir::{
        IrError, capability::plan_translation_with_history, continuity::ContinuityBinding,
        responses,
    },
};
use std::io::{Read, Write};

fn read(input: &mut impl Read, sequence: u64) -> Result<Operation, IrError> {
    let mut prefix = [0; 4];
    input
        .read_exact(&mut prefix)
        .map_err(|_| IrError::InvalidEventOrder)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(IrError::SizeLimit);
    }
    let mut bytes = vec![0; length];
    input
        .read_exact(&mut bytes)
        .map_err(|_| IrError::InvalidEventOrder)?;
    let value = crate::adapters::json::decode(&bytes)?;
    let request: Request =
        serde_json::from_value(value).map_err(|_| IrError::UnsupportedVersion)?;
    if request.protocol != PROTOCOL || request.sequence != sequence {
        return Err(IrError::UnsupportedVersion);
    }
    Ok(request.operation)
}
fn write(output: &mut impl Write, sequence: u64, value: ResultValue) -> Result<(), IrError> {
    let bytes = serde_json::to_vec(&Reply {
        protocol: PROTOCOL.into(),
        sequence,
        value,
    })
    .map_err(|_| IrError::InvalidEventOrder)?;
    if bytes.len() > MAX_FRAME {
        return Err(IrError::SizeLimit);
    }
    output
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| output.write_all(&bytes))
        .and_then(|_| output.flush())
        .map_err(|_| IrError::InvalidEventOrder)
}
fn managed(value: crate::adapters::managed::ManagedOutput) -> ResultValue {
    ResultValue::Managed {
        value: Box::new(ManagedResult {
            response: value.response,
            native: value.native,
            outcome: value.outcome,
            accounting: value.accounting,
        }),
    }
}

/// Serve one request-scoped codec process. Diagnostics never include payloads.
pub fn serve(input: &mut impl Read, output: &mut impl Write) -> Result<(), IrError> {
    write(
        output,
        0,
        ResultValue::Ready {
            apis: vec![
                crate::ir::ApiProtocol::Responses,
                crate::ir::ApiProtocol::Messages,
                crate::ir::ApiProtocol::ChatCompletions,
                crate::ir::ApiProtocol::GeminiInteractions,
            ],
            replay_versions: vec![1],
        },
    )?;
    let Operation::Prepare { value: prepare } = read(input, 1)? else {
        return Err(IrError::InvalidEventOrder);
    };
    if prepare.max_output_bytes == 0 || prepare.max_output_bytes > 64 * 1024 * 1024 {
        return Err(IrError::SizeLimit);
    }
    let history = prepare.history()?;
    let request = responses::decode(prepare.request.clone(), None)?;
    let route = prepare.route.snapshot()?;
    let plan = plan_translation_with_history(
        &request,
        &ContinuityBinding {
            route,
            scope: "codec-request".into(),
        },
        &history,
    )?;
    if prepare.managed {
        let adapter = ManagedAdapter::encode(&request, &plan, &history)?;
        if prepare.pending_tools {
            adapter.validate_pending_controls(&history)?;
        }
        write(
            output,
            1,
            ResultValue::Prepared {
                payload: adapter.payload().clone(),
            },
        )?;
        match read(input, 2)? {
            Operation::Json { body, response_id } => write(
                output,
                2,
                managed(adapter.decode_bytes(body.as_bytes(), &response_id)?),
            ),
            Operation::Stream { response_id } => {
                let mut stream = adapter.stream(prepare.max_output_bytes, response_id);
                write(
                    output,
                    2,
                    ResultValue::Progress {
                        events: vec![],
                        complete: false,
                        accounting: None,
                    },
                )?;
                let mut sequence = 3;
                loop {
                    match read(input, sequence)? {
                        Operation::Event { event, data } => {
                            stream.event(SseEvent { event, data })?;
                            write(
                                output,
                                sequence,
                                ResultValue::Progress {
                                    events: stream.take_progress(),
                                    complete: stream.is_complete(),
                                    accounting: Some(stream.accounting()),
                                },
                            )?;
                        }
                        Operation::Finish => {
                            return write(output, sequence, managed(stream.finish()?));
                        }
                        _ => return Err(IrError::InvalidEventOrder),
                    }
                    sequence += 1;
                }
            }
            _ => Err(IrError::InvalidEventOrder),
        }
    } else {
        if !history.segments.is_empty() || prepare.pending_tools {
            return Err(IrError::ContinuityMismatch);
        }
        let mut adapter = PreparedAdapter::encode(&request, &plan)?;
        write(
            output,
            1,
            ResultValue::Prepared {
                payload: adapter.take_payload(),
            },
        )?;
        match read(input, 2)? {
            Operation::Json { body, .. } => write(
                output,
                2,
                ResultValue::Json {
                    response: adapter.decode_bytes(body.as_bytes())?,
                },
            ),
            Operation::Stream { .. } => {
                let mut stream = adapter.stream(prepare.max_output_bytes)?;
                write(
                    output,
                    2,
                    ResultValue::Progress {
                        events: vec![],
                        complete: false,
                        accounting: None,
                    },
                )?;
                let mut sequence = 3;
                loop {
                    match read(input, sequence)? {
                        Operation::Event { event, data } => {
                            let events = stream.event(SseEvent { event, data })?;
                            write(
                                output,
                                sequence,
                                ResultValue::Progress {
                                    events,
                                    complete: stream.is_complete(),
                                    accounting: None,
                                },
                            )?;
                        }
                        Operation::Finish => {
                            stream.finish()?;
                            return write(output, sequence, ResultValue::Finished);
                        }
                        _ => return Err(IrError::InvalidEventOrder),
                    }
                    sequence += 1;
                }
            }
            _ => Err(IrError::InvalidEventOrder),
        }
    }
}
