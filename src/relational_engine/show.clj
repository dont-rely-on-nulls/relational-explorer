(ns relational-engine.show
  (:require [clojure.java.io]
            [clojure.xml :as xml]
            [clojure.edn :as edn])
  (:import (com.github.freva.asciitable AsciiTable)
           (com.github.freva.asciitable Column)
           (java.net Socket)
           (org.xml.sax InputSource)
           (java.io InputStreamReader OutputStreamWriter StringReader)
           (clojure.lang LineNumberingPushbackReader)))

(let [socket (new Socket "127.0.0.1" 7524)
      in (new LineNumberingPushbackReader
              (new InputStreamReader (. socket (getInputStream))))
      out (new OutputStreamWriter (. socket (getOutputStream)))
      msg "PROJECT user/first-name, user/last-name FROM user"]
  (clojure.pprint/pprint
   (binding [*out* out]
     (pr msg)
     (flush)
     (xml/parse (new InputSource (new StringReader (str "<?xml version='1.0' encoding='utf-8'?>" (read in))))))))


(let* [data (to-array-2d [["1" "2"]
                          ["1" "2"]
                          ["1" "2"]])
       table-internal1 (AsciiTable/getTable data)
       data2 (to-array-2d [["1" "2"]
                          ["1" "2"]
                          ["1" table-internal1]])
       table-internal2 (AsciiTable/getTable data2)
       data3 (to-array-2d [["1" "2" "3"]
                           ["1" "2" "3"]
                           ["1" "2" table-internal2]])
       x (AsciiTable/getTable data3)]
      (println x))

(defrecord Person [fname lname address])
(defrecord Address [street num])

(let [data [(Person. "Stu" "Halloway" (Address. "Saint James St." 123))
            (Person. "Marcos" "Magueta" (Address. "Saint Jerome St." 203))
            (Person. "Alfred" "Parker" (Address. "Saint Stephen St." 842))]
      java-cast (comp java.util.Arrays/asList to-array)]
  (-> (AsciiTable/getTable
       AsciiTable/FANCY_ASCII
       (java-cast data)
       (-> [(.with (.header (new Column) "fname:String") :fname)
            (.with (.header (new Column) "lname:String") :lname)
            (.with (.header (new Column) "Address:String")
                   (fn [x] (AsciiTable/getTable
                            AsciiTable/FANCY_ASCII
                            (java.util.Arrays/asList (to-array [(:address x)]))
                            (-> [(.with (.header (new Column) "street:String")
                                        :street)
                                 (.with (.header (new Column) "num:Integer")
                                        (comp str :num))]
                                java-cast))))]
           java-cast))
      println))
