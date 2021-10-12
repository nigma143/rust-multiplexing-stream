# rust-multiplexing-stream
Example reverse access proxy

```
               DMZ                            |            INTRANET                               
                                              |                                      
clients (tls-socket)===> reverse-access-proxy <===(multiplexor-over-socket) proxy
                                              |                              
```

Current example implemented as client clients (https)===> proxy <===(ws-over-http) server  
Used multiplexor protocol: https://github.com/AArnott/Nerdbank.Streams 
